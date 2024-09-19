use crate::runtime::{
    execution::ExecutionState,
    task::{clock::VectorClock, Task, TaskId},
};
use serde::Serialize;
use std::thread_local;
use std::cell::RefCell;
use std::collections::HashMap;

// TODO: make functions here no-ops/inlined if a compile-time feature for
//       Shuttle is not enabled, to make sure there is no performance impact
//       at all
// TODO: the types defined here with `derive(Serialize)` are all parsed from
//       JSON output by Shuttle Explorer; if any changes are made, they should
//       also be reflected in the parsing
// TODO: introduce version numbers to make sure breaking changes are noticed

thread_local! {
    static ANNOTATION_STATE: RefCell<Option<AnnotationState>> = RefCell::new(None);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ObjectId(usize);

pub(crate) const DUMMY_OBJECT_ID: ObjectId = ObjectId(usize::MAX);

impl ObjectId {
    fn is_dummy(&self) -> bool {
        *self == DUMMY_OBJECT_ID
    }
}

#[derive(Serialize)]
struct FileInfo {
    path: String,
}

#[derive(Serialize)]
struct FunctionInfo {
    name: String,
}

#[derive(Serialize)]
struct Frame(
    usize, // file
    usize, // function
    usize, // line
    usize, // col
);

#[derive(Serialize)]
struct ObjectInfo {
    created_by: TaskId,
    created_at: usize,
    name: Option<String>,
    kind: Option<String>,
    // count of interactions?
    // ...?
}

#[derive(Serialize)]
struct TaskInfo {
    created_by: TaskId,
    first_step: usize,
    last_step: usize,
    name: Option<String>,
}

#[derive(Debug, Serialize)]
enum AnnotationEvent {
    SemaphoreCreated(ObjectId),
    SemaphoreClosed(ObjectId),
    SemaphoreAcquireFast(ObjectId, usize),
    SemaphoreAcquireBlocked(ObjectId, usize),
    SemaphoreAcquireUnblocked(ObjectId, TaskId, usize),
    SemaphoreTryAcquire(ObjectId, usize, bool),
    SemaphoreRelease(ObjectId, usize),

    TaskCreated(TaskId, bool),
    TaskTerminated,

    Random,
    Tick, // just to record a backtrace ...
    Schedule( // TODO: maybe these should be part of Tick somehow?
        Vec<TaskId>, // runnable
    ),
}

#[derive(Default, Serialize)]
struct AnnotationState {
    files: Vec<FileInfo>,
    #[serde(skip)] path_to_file: HashMap<String, usize>,
    functions: Vec<FunctionInfo>,
    #[serde(skip)] name_to_function: HashMap<String, usize>,
    objects: Vec<ObjectInfo>,
    tasks: Vec<TaskInfo>,
    events: Vec<( // TODO: make into a struct EventInfo
        TaskId, // which task did something/yielded?
        Option<Vec<Frame>>, // backtrace
        AnnotationEvent, // what happened?
        Option<VectorClock>, // causal relations with other tasks
    )>,
    #[serde(skip)] last_task_id: Option<TaskId>,
    #[serde(skip)] last_clock: Option<VectorClock>,
    #[serde(skip)] max_task_id: Option<TaskId>,
    // task_names: Vec<String>, // TODO: gather during `next_task`
}

fn with_state<R, F: FnOnce(&mut AnnotationState) -> R>(f: F) -> Option<R> {
    ANNOTATION_STATE.with(|cell| {
        let mut bw = cell.borrow_mut();
        let state = bw.as_mut()?;
        Some(f(state))
    })
}

pub(crate) fn start_annotations() {
    ANNOTATION_STATE.with(|cell| {
        let mut bw = cell.borrow_mut();
        assert!(bw.is_none(), "annotations already started");
        let mut state: AnnotationState = Default::default();
        state.last_task_id = Some(0.into());
        //state.last_clock = Some(VectorClock::new());
        *bw = Some(state);
    });
}

pub(crate) fn stop_annotations() {
    ANNOTATION_STATE.with(|cell| {
        let mut bw = cell.borrow_mut();
        let state = bw.take().expect("annotations not started");
        if state.max_task_id.is_none() {
            // nothing to output
            return;
        };
        let json = serde_json::to_string(&state).unwrap();
        // TODO: configure
        std::fs::write("/Users/aubily/workplace/shuttle-clients/annotated.json", json).unwrap();
    });
}

fn record_object() -> ObjectId {
    with_state(|state| {
        let id = ObjectId(state.objects.len());
        state.objects.push(ObjectInfo {
            created_by: state.last_task_id.unwrap(),
            created_at: state.events.len(),
            name: None,
            kind: None,
        });
        id
    }).unwrap_or(DUMMY_OBJECT_ID)
}

fn record_event(event: AnnotationEvent) {
    use once_cell::sync::Lazy;
    use regex::Regex;
    // TODO: don't hardcode ./src/ here
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"([0-9]+): ([^\n]+)\n +at (\./src/[^:]+):([0-9]+):([0-9]+)\b").unwrap());
    with_state(move |state| {
        let task_id = state.last_task_id.expect("no last task ID?"); // unwrap_or(crate::current::me());
        /*if let Some(last_task_id) = state.last_task_id {
            assert_eq!(task_id, last_task_id);
        }*/

        let task_id_num = usize::from(task_id);
        assert!(task_id_num < state.tasks.len());
        state.tasks[task_id_num].first_step = state.tasks[task_id_num].first_step.min(state.events.len());
        state.tasks[task_id_num].last_step = state.tasks[task_id_num].last_step.max(state.events.len());

        /*
        use std::backtrace::Backtrace;
        let bt = Backtrace::capture(); // TODO: config to disable; also disable for Phobos because nightly Rust
        let bt_str = format!("{bt}");
        let mut info = Vec::new();
        for groups in RE.captures_iter(&bt_str) {
            let _num = groups.get(1).unwrap().as_str();
            let function_name = groups.get(2).unwrap().as_str();
            let path = groups.get(3).unwrap().as_str();
            let line_str = groups.get(4).unwrap().as_str();
            let col_str = groups.get(5).unwrap().as_str();

            let path_idx = state.path_to_file.entry(path.to_string())
                .or_insert_with(|| {
                    let idx = state.files.len();
                    state.files.push(FileInfo {
                        path: path.to_string(),
                    });
                    idx
                });
            let function_idx = state.name_to_function.entry(function_name.to_string())
                .or_insert_with(|| {
                    let idx = state.functions.len();
                    state.functions.push(FunctionInfo {
                        name: function_name.to_string(),
                    });
                    idx
                });
            info.push(Frame(
                *path_idx, // file
                *function_idx, // function
                line_str.parse::<usize>().unwrap(), // line
                col_str.parse::<usize>().unwrap(), // col
            ));
        }
        */
        state.events.push((
            task_id,
            None, /*if info.is_empty() {
                None
            } else {
                Some(info)
            }, */
            event,
            ExecutionState::try_with(|state| state.get_clock(task_id).clone()),
        ))
    });
}

pub(crate) fn record_semaphore_created() -> ObjectId {
    let object_id = record_object();
    record_event(AnnotationEvent::SemaphoreCreated(object_id));
    object_id
}

pub(crate) fn record_semaphore_closed(object_id: ObjectId) {
    record_event(AnnotationEvent::SemaphoreClosed(object_id));
}

pub(crate) fn record_semaphore_acquire_fast(object_id: ObjectId, num_permits: usize) {
    record_event(AnnotationEvent::SemaphoreAcquireFast(object_id, num_permits));
}

pub(crate) fn record_semaphore_acquire_blocked(object_id: ObjectId, num_permits: usize) {
    record_event(AnnotationEvent::SemaphoreAcquireBlocked(object_id, num_permits));
}

pub(crate) fn record_semaphore_acquire_unblocked(object_id: ObjectId, unblocked_task_id: TaskId, num_permits: usize) {
    record_event(AnnotationEvent::SemaphoreAcquireUnblocked(object_id, unblocked_task_id, num_permits));
}

pub(crate) fn record_semaphore_try_acquire(object_id: ObjectId, num_permits: usize, successful: bool) {
    record_event(AnnotationEvent::SemaphoreTryAcquire(object_id, num_permits, successful));
}

pub(crate) fn record_semaphore_release(object_id: ObjectId, num_permits: usize) {
    record_event(AnnotationEvent::SemaphoreRelease(object_id, num_permits));
}

pub(crate) fn record_task_created(task_id: TaskId, future: bool) {
    with_state(move |state| {
        assert_eq!(state.tasks.len(), usize::from(task_id));
        state.tasks.push(TaskInfo {
            created_by: state.last_task_id.unwrap(),
            first_step: usize::MAX,
            last_step: 0,
            name: None,
        });
    });
    record_event(AnnotationEvent::TaskCreated(task_id, future));
}

pub(crate) fn record_task_terminated() {
    record_event(AnnotationEvent::TaskTerminated);
}

pub(crate) fn record_name_for_object(object_id: ObjectId, name: Option<&str>, kind: Option<&str>) {
    with_state(move |state| {
        if let Some(object_info) = state.objects.get_mut(object_id.0) {
            if name.is_some() {
                object_info.name = name.map(|name| name.to_string());
            }
            if kind.is_some() {
                object_info.kind = kind.map(|kind| kind.to_string());
            }
        } // TODO: else panic? warn?
    });
}

pub(crate) fn record_name_for_task(task_id: TaskId, name: &crate::current::TaskName) {
    with_state(|state| {
        if let Some(task_info) = state.tasks.get_mut(usize::from(task_id)) {
            let name: &String = name.into();
            task_info.name = Some(name.to_string());
        } // TODO: else panic? warn?
    });
}

pub(crate) fn record_random() {
    record_event(AnnotationEvent::Random);
}

pub(crate) fn record_schedule(choice: TaskId, runnable_tasks: &[&Task]) {
    with_state(|state| {
        let choice_id_num = usize::from(choice);
        state.tasks[choice_id_num].first_step = state.tasks[choice_id_num].first_step.min(state.events.len());
        state.tasks[choice_id_num].last_step = state.tasks[choice_id_num].last_step.max(state.events.len());
        let runnable_ids = runnable_tasks
            .iter()
            .map(|task| task.id())
            .collect::<Vec<_>>();
        state.events.push((
            choice,
            None,
            AnnotationEvent::Schedule(runnable_ids),
            None, // VectorClock::new(),
        ));
        state.last_task_id = Some(choice);
        state.max_task_id = state.max_task_id.max(Some(choice));
    });
}

pub(crate) fn record_tick() {
    record_event(AnnotationEvent::Tick);
}

pub trait WithName {
    fn with_name_and_kind(self, name: Option<&str>, kind: Option<&str>) -> Self;

    fn with_name(self, name: &str) -> Self
    where Self: Sized
    {
        self.with_name_and_kind(Some(name), None)
    }

    fn with_kind(self, kind: &str) -> Self
    where Self: Sized
    {
        self.with_name_and_kind(None, Some(kind))
    }
}
