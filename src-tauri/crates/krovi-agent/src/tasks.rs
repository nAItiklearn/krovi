//! The task queue: one task per step that has been (or is waiting to be) run. Pure bookkeeping;
//! the host does the work and reports back with [`TaskQueue::set_status`].
//!
//! ```text
//! Pending ──► Running ──► Done | Failed(msg)
//!    │           ▲
//!    ▼           │
//! AwaitingConfirmation ──► Cancelled      (Pending ──► Cancelled)
//! ```
//!
//! Done, Failed and Cancelled are final. History is capped: when the queue is full the oldest
//! *finished* task is dropped; tasks that are still pending, waiting or running are never
//! evicted.

use std::collections::VecDeque;
use std::fmt;

use serde::Serialize;

/// Most tasks kept (running, waiting and finished together).
pub const HISTORY_CAP: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "message", rename_all = "kebab-case")]
pub enum TaskStatus {
    Pending,
    AwaitingConfirmation,
    Running,
    Done,
    Failed(String),
    Cancelled,
}

impl TaskStatus {
    /// Done, Failed or Cancelled: nothing more will happen to it.
    pub fn is_final(&self) -> bool {
        matches!(
            self,
            TaskStatus::Done | TaskStatus::Failed(_) | TaskStatus::Cancelled
        )
    }

    fn name(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::AwaitingConfirmation => "awaiting confirmation",
            TaskStatus::Running => "running",
            TaskStatus::Done => "done",
            TaskStatus::Failed(_) => "failed",
            TaskStatus::Cancelled => "cancelled",
        }
    }

    fn can_become(&self, next: &TaskStatus) -> bool {
        use TaskStatus::*;
        matches!(
            (self, next),
            (Pending, AwaitingConfirmation | Running | Cancelled)
                | (AwaitingConfirmation, Pending | Running | Cancelled)
                | (Running, Done | Failed(_))
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: u64,
    pub plan_id: String,
    pub step_id: String,
    /// The step's summary, so the list reads on its own.
    pub summary: String,
    pub status: TaskStatus,
    /// Epoch milliseconds.
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskError {
    NotFound,
    /// `from` -> `to` is not allowed (for example cancelling a task that is already running).
    IllegalTransition {
        from: &'static str,
        to: &'static str,
    },
    /// Every slot holds a task that is still active.
    Full,
    /// New tasks start Pending, AwaitingConfirmation or Running.
    BadInitialStatus,
}

impl fmt::Display for TaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskError::NotFound => write!(f, "That task no longer exists."),
            TaskError::IllegalTransition { from, to } => {
                write!(f, "A task that is {from} cannot become {to}.")
            }
            TaskError::Full => write!(f, "Too many tasks are waiting. Cancel some first."),
            TaskError::BadInitialStatus => write!(f, "A new task cannot start in that state."),
        }
    }
}

impl std::error::Error for TaskError {}

#[derive(Debug, Default)]
pub struct TaskQueue {
    tasks: VecDeque<Task>,
    next_id: u64,
}

impl TaskQueue {
    pub fn new() -> Self {
        Self {
            tasks: VecDeque::new(),
            next_id: 1,
        }
    }

    /// Add a task. If the queue is full the oldest finished task is dropped to make room;
    /// if none is finished the new task is refused ([`TaskError::Full`]).
    pub fn push(
        &mut self,
        plan_id: &str,
        step_id: &str,
        summary: &str,
        status: TaskStatus,
        now: u64,
    ) -> Result<u64, TaskError> {
        if !matches!(
            status,
            TaskStatus::Pending | TaskStatus::AwaitingConfirmation | TaskStatus::Running
        ) {
            return Err(TaskError::BadInitialStatus);
        }
        if self.next_id == 0 {
            self.next_id = 1;
        }
        while self.tasks.len() >= HISTORY_CAP {
            match self.tasks.iter().position(|t| t.status.is_final()) {
                Some(i) => {
                    self.tasks.remove(i);
                }
                None => return Err(TaskError::Full),
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        self.tasks.push_back(Task {
            id,
            plan_id: plan_id.to_string(),
            step_id: step_id.to_string(),
            summary: summary.to_string(),
            status,
            created_at: now,
            updated_at: now,
        });
        Ok(id)
    }

    pub fn get(&self, id: u64) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    /// The task made for a plan's step, if any.
    pub fn for_step(&self, plan_id: &str, step_id: &str) -> Option<&Task> {
        self.tasks
            .iter()
            .find(|t| t.plan_id == plan_id && t.step_id == step_id)
    }

    /// Move a task to a new status (checked against the diagram above). Returns the updated task.
    pub fn set_status(&mut self, id: u64, status: TaskStatus, now: u64) -> Result<Task, TaskError> {
        let task = self
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(TaskError::NotFound)?;
        if !task.status.can_become(&status) {
            return Err(TaskError::IllegalTransition {
                from: task.status.name(),
                to: status.name(),
            });
        }
        task.status = status;
        task.updated_at = now;
        Ok(task.clone())
    }

    /// Cancel a task that has not started. A running or finished task cannot be cancelled.
    pub fn cancel(&mut self, id: u64, now: u64) -> Result<Task, TaskError> {
        self.set_status(id, TaskStatus::Cancelled, now)
    }

    /// Newest first.
    pub fn list(&self) -> Vec<Task> {
        self.tasks.iter().rev().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Tasks that are pending, waiting for confirmation or running.
    pub fn active_count(&self) -> usize {
        self.tasks.iter().filter(|t| !t.status.is_final()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push(q: &mut TaskQueue, status: TaskStatus) -> u64 {
        q.push("p1", "s1", "Do a thing", status, 1000).unwrap()
    }

    #[test]
    fn ids_increase_and_list_is_newest_first() {
        let mut q = TaskQueue::new();
        let a = push(&mut q, TaskStatus::Pending);
        let b = push(&mut q, TaskStatus::Pending);
        assert!(b > a);
        let ids: Vec<u64> = q.list().iter().map(|t| t.id).collect();
        assert_eq!(ids, [b, a]);
        assert_eq!(q.len(), 2);
        assert!(!q.is_empty());
    }

    #[test]
    fn happy_path_to_done() {
        let mut q = TaskQueue::new();
        let id = push(&mut q, TaskStatus::Pending);
        assert_eq!(
            q.set_status(id, TaskStatus::Running, 2000)
                .unwrap()
                .updated_at,
            2000
        );
        let t = q.set_status(id, TaskStatus::Done, 3000).unwrap();
        assert_eq!(
            (t.status, t.created_at, t.updated_at),
            (TaskStatus::Done, 1000, 3000)
        );
    }

    #[test]
    fn failure_keeps_its_message() {
        let mut q = TaskQueue::new();
        let id = push(&mut q, TaskStatus::Running);
        let t = q
            .set_status(
                id,
                TaskStatus::Failed("Windows could not open it".into()),
                5,
            )
            .unwrap();
        assert_eq!(
            t.status,
            TaskStatus::Failed("Windows could not open it".into())
        );
        assert!(t.status.is_final());
    }

    #[test]
    fn confirmation_flow() {
        let mut q = TaskQueue::new();
        let id = push(&mut q, TaskStatus::AwaitingConfirmation);
        assert_eq!(q.active_count(), 1);
        q.set_status(id, TaskStatus::Running, 1).unwrap();
        q.set_status(id, TaskStatus::Done, 2).unwrap();
        assert_eq!(q.active_count(), 0);
    }

    #[test]
    fn cancel_only_before_it_starts() {
        let mut q = TaskQueue::new();
        let pending = push(&mut q, TaskStatus::Pending);
        let waiting = push(&mut q, TaskStatus::AwaitingConfirmation);
        let running = push(&mut q, TaskStatus::Running);
        let done = push(&mut q, TaskStatus::Running);
        q.set_status(done, TaskStatus::Done, 1).unwrap();

        assert_eq!(q.cancel(pending, 9).unwrap().status, TaskStatus::Cancelled);
        assert_eq!(q.cancel(waiting, 9).unwrap().status, TaskStatus::Cancelled);
        assert_eq!(
            q.cancel(running, 9),
            Err(TaskError::IllegalTransition {
                from: "running",
                to: "cancelled"
            })
        );
        assert_eq!(
            q.cancel(done, 9),
            Err(TaskError::IllegalTransition {
                from: "done",
                to: "cancelled"
            })
        );
        assert_eq!(
            q.cancel(pending, 9),
            Err(TaskError::IllegalTransition {
                from: "cancelled",
                to: "cancelled"
            })
        );
        assert_eq!(q.cancel(999, 9), Err(TaskError::NotFound));
        // a failed cancel changes nothing
        assert_eq!(q.get(running).unwrap().status, TaskStatus::Running);
        assert_eq!(q.get(running).unwrap().updated_at, 1000);
    }

    #[test]
    fn finished_tasks_are_final() {
        let mut q = TaskQueue::new();
        for last in [TaskStatus::Done, TaskStatus::Failed("x".into())] {
            let id = push(&mut q, TaskStatus::Running);
            q.set_status(id, last, 1).unwrap();
            for next in [
                TaskStatus::Running,
                TaskStatus::Pending,
                TaskStatus::Done,
                TaskStatus::Cancelled,
            ] {
                assert!(q.set_status(id, next, 2).is_err());
            }
        }
        let id = push(&mut q, TaskStatus::Pending);
        q.cancel(id, 1).unwrap();
        assert!(q.set_status(id, TaskStatus::Running, 2).is_err());
    }

    #[test]
    fn illegal_jumps_are_refused() {
        let mut q = TaskQueue::new();
        let id = push(&mut q, TaskStatus::Pending);
        assert!(
            q.set_status(id, TaskStatus::Done, 1).is_err(),
            "must run first"
        );
        assert!(q.set_status(id, TaskStatus::Failed("x".into()), 1).is_err());
        assert!(q.set_status(id, TaskStatus::Pending, 1).is_err());
    }

    #[test]
    fn new_tasks_cannot_start_finished() {
        let mut q = TaskQueue::new();
        for bad in [
            TaskStatus::Done,
            TaskStatus::Cancelled,
            TaskStatus::Failed("x".into()),
        ] {
            assert_eq!(
                q.push("p", "s", "x", bad, 0),
                Err(TaskError::BadInitialStatus)
            );
        }
        assert!(q.is_empty());
    }

    #[test]
    fn history_is_capped_at_50_dropping_the_oldest_finished() {
        let mut q = TaskQueue::new();
        let mut ids = Vec::new();
        for _ in 0..HISTORY_CAP {
            let id = push(&mut q, TaskStatus::Running);
            q.set_status(id, TaskStatus::Done, 1).unwrap();
            ids.push(id);
        }
        assert_eq!(q.len(), 50);
        let extra = push(&mut q, TaskStatus::Pending);
        assert_eq!(q.len(), 50);
        assert!(q.get(ids[0]).is_none(), "oldest finished task dropped");
        assert!(q.get(ids[1]).is_some());
        assert!(q.get(extra).is_some());
        for _ in 0..200 {
            let id = push(&mut q, TaskStatus::Running);
            q.set_status(id, TaskStatus::Done, 1).unwrap();
            assert!(q.len() <= HISTORY_CAP);
        }
        // the one still pending survived all of it
        assert_eq!(q.get(extra).unwrap().status, TaskStatus::Pending);
    }

    #[test]
    fn active_tasks_are_never_evicted_and_a_full_queue_refuses() {
        let mut q = TaskQueue::new();
        for _ in 0..HISTORY_CAP {
            push(&mut q, TaskStatus::AwaitingConfirmation);
        }
        assert_eq!(
            q.push("p", "s", "x", TaskStatus::Pending, 0),
            Err(TaskError::Full)
        );
        assert_eq!(q.len(), HISTORY_CAP);
        assert_eq!(q.active_count(), HISTORY_CAP);
        // cancelling one frees a slot
        let first = q.list().last().unwrap().id;
        q.cancel(first, 1).unwrap();
        assert!(q.push("p", "s", "x", TaskStatus::Pending, 0).is_ok());
        assert!(q.get(first).is_none());
    }

    #[test]
    fn lookup_by_plan_and_step() {
        let mut q = TaskQueue::new();
        let a = q.push("p1", "s1", "a", TaskStatus::Pending, 0).unwrap();
        let b = q.push("p1", "s2", "b", TaskStatus::Pending, 0).unwrap();
        q.push("p2", "s1", "c", TaskStatus::Pending, 0).unwrap();
        assert_eq!(q.for_step("p1", "s1").unwrap().id, a);
        assert_eq!(q.for_step("p1", "s2").unwrap().id, b);
        assert!(q.for_step("p3", "s1").is_none());
    }

    #[test]
    fn json_shape() {
        let mut q = TaskQueue::new();
        let id = push(&mut q, TaskStatus::Pending);
        let v = serde_json::to_value(q.get(id).unwrap()).unwrap();
        assert_eq!(v["planId"], "p1");
        assert_eq!(v["stepId"], "s1");
        assert_eq!(v["createdAt"], 1000);
        assert_eq!(v["status"], serde_json::json!({ "state": "pending" }));
        q.set_status(id, TaskStatus::Running, 1).unwrap();
        let t = q
            .set_status(id, TaskStatus::Failed("boom".into()), 2)
            .unwrap();
        assert_eq!(
            serde_json::to_value(&t.status).unwrap(),
            serde_json::json!({ "state": "failed", "message": "boom" })
        );
        assert_eq!(
            serde_json::to_value(TaskStatus::AwaitingConfirmation).unwrap(),
            serde_json::json!({ "state": "awaiting-confirmation" })
        );
    }
}
