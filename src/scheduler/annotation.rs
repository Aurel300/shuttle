use crate::annotations::{record_schedule, record_random, start_annotations, stop_annotations};
use crate::runtime::task::{Task, TaskId};
use crate::scheduler::{Schedule, Scheduler};

/// TODO: document
#[derive(Debug)]
pub struct AnnotationScheduler<S: ?Sized + Scheduler>(Box<S>);

impl<S: Scheduler> AnnotationScheduler<S> {
    /// TODO: document
    pub fn new(inner: S) -> Self {
        start_annotations();
        Self(Box::new(inner))
    }
}

impl<S: Scheduler> Scheduler for AnnotationScheduler<S> {
    fn new_execution(&mut self) -> Option<Schedule> {
        self.0.new_execution()
    }

    fn next_task(
        &mut self,
        runnable_tasks: &[&Task],
        current_task: Option<TaskId>,
        is_yielding: bool,
    ) -> Option<TaskId> {
        let choice = self.0.next_task(runnable_tasks, current_task, is_yielding)?;
        record_schedule(choice, runnable_tasks);
        Some(choice)
    }

    fn next_u64(&mut self) -> u64 {
        record_random();
        self.0.next_u64()
    }
}

impl<S: ?Sized + Scheduler> Drop for AnnotationScheduler<S> {
    fn drop(&mut self) {
        stop_annotations();
    }
}
