//! A session ties plans to tasks and decides, in one auditable place, which steps may run.
//!
//! The host calls [`Session::begin_run`] with the plan id and the step ids the user confirmed,
//! gets back the steps to execute, runs each one, and reports with [`Session::finish`]. Plans
//! live only here (in memory): the UI refers to them by id, so it can never smuggle an action
//! into a run.
//!
//! Guarantees (all tested):
//! * a `Blocked` step is never dispatched, whatever the caller passes;
//! * a `Confirm` step is dispatched only if its id is in `confirmed`;
//! * every step is dispatched at most once per plan (running it twice is a no-op);
//! * a cancelled step is never dispatched.

use std::collections::VecDeque;
use std::fmt;

use crate::plan::{Plan, Risk, Step};
use crate::tasks::{Task, TaskError, TaskQueue, TaskStatus};

/// Plans remembered; older ones are forgotten (running one then says it expired).
pub const PLAN_CAP: usize = 16;

/// A step the host must now execute, and the task to report its outcome on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispatch {
    pub task_id: u64,
    pub step: Step,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct RunOutcome {
    /// Tasks created or changed by this call, in plan order.
    pub touched: Vec<u64>,
    /// What to execute now, in plan order.
    pub dispatches: Vec<Dispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    UnknownPlan,
    Task(TaskError),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::UnknownPlan => write!(f, "That plan has expired. Ask again."),
            SessionError::Task(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<TaskError> for SessionError {
    fn from(e: TaskError) -> Self {
        SessionError::Task(e)
    }
}

#[derive(Debug, Default)]
pub struct Session {
    plans: VecDeque<Plan>,
    tasks: TaskQueue,
}

impl Session {
    pub fn new() -> Self {
        Self {
            plans: VecDeque::new(),
            tasks: TaskQueue::new(),
        }
    }

    /// Remember a plan (replacing one with the same id); the oldest is forgotten past the cap.
    pub fn add_plan(&mut self, plan: Plan) {
        self.plans.retain(|p| p.id != plan.id);
        self.plans.push_back(plan);
        while self.plans.len() > PLAN_CAP {
            self.plans.pop_front();
        }
    }

    pub fn plan(&self, id: &str) -> Option<&Plan> {
        self.plans.iter().find(|p| p.id == id)
    }

    /// Decide what runs now. See the module docs for the guarantees.
    pub fn begin_run(
        &mut self,
        plan_id: &str,
        confirmed: &[String],
        now: u64,
    ) -> Result<RunOutcome, SessionError> {
        let plan = self
            .plan(plan_id)
            .cloned()
            .ok_or(SessionError::UnknownPlan)?;
        let mut out = RunOutcome::default();
        if let Err(e) = self.run_steps(&plan, confirmed, now, &mut out) {
            // never leave a task "running" that will not be executed
            for d in &out.dispatches {
                let _ = self.tasks.set_status(
                    d.task_id,
                    TaskStatus::Failed("Could not start.".into()),
                    now,
                );
            }
            return Err(e);
        }
        Ok(out)
    }

    fn run_steps(
        &mut self,
        plan: &Plan,
        confirmed: &[String],
        now: u64,
        out: &mut RunOutcome,
    ) -> Result<(), SessionError> {
        for step in &plan.steps {
            let allowed = match step.risk {
                Risk::Blocked(_) => continue,
                Risk::Safe => true,
                Risk::Confirm => confirmed.contains(&step.id),
            };
            let existing = self
                .tasks
                .for_step(&plan.id, &step.id)
                .map(|t| (t.id, t.status.clone()));
            match existing {
                None => {
                    let initial = if allowed {
                        TaskStatus::Running
                    } else {
                        TaskStatus::AwaitingConfirmation
                    };
                    let id = self
                        .tasks
                        .push(&plan.id, &step.id, &step.summary, initial, now)?;
                    out.touched.push(id);
                    if allowed {
                        out.dispatches.push(Dispatch {
                            task_id: id,
                            step: step.clone(),
                        });
                    }
                }
                Some((id, TaskStatus::AwaitingConfirmation)) if allowed => {
                    self.tasks.set_status(id, TaskStatus::Running, now)?;
                    out.touched.push(id);
                    out.dispatches.push(Dispatch {
                        task_id: id,
                        step: step.clone(),
                    });
                }
                // already running, finished, cancelled or still waiting for the user
                Some(_) => {}
            }
        }
        Ok(())
    }

    /// Report the outcome of a dispatched step.
    pub fn finish(
        &mut self,
        task_id: u64,
        result: Result<(), String>,
        now: u64,
    ) -> Result<Task, TaskError> {
        let status = match result {
            Ok(()) => TaskStatus::Done,
            Err(msg) => TaskStatus::Failed(msg),
        };
        self.tasks.set_status(task_id, status, now)
    }

    /// Cancel a waiting task (the Cancel button on a confirmation card).
    pub fn cancel(&mut self, task_id: u64, now: u64) -> Result<Task, TaskError> {
        self.tasks.cancel(task_id, now)
    }

    pub fn task(&self, id: u64) -> Option<Task> {
        self.tasks.get(id).cloned()
    }

    /// Newest first.
    pub fn tasks(&self) -> Vec<Task> {
        self.tasks.list()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Action;
    use crate::tasks::HISTORY_CAP;

    fn step(id: &str, risk: Risk) -> Step {
        Step {
            id: id.into(),
            action: Action::Note {
                body: format!("body of {id}"),
            },
            summary: format!("Step {id}"),
            risk,
        }
    }

    fn plan(id: &str, steps: Vec<Step>) -> Plan {
        Plan {
            id: id.into(),
            steps,
            suggestions: vec![],
        }
    }

    fn mixed() -> Plan {
        plan(
            "p1",
            vec![
                step("s1", Risk::Safe),
                step("s2", Risk::Confirm),
                step("s3", Risk::Blocked("nope".into())),
                step("s4", Risk::Confirm),
            ],
        )
    }

    fn ids(o: &RunOutcome) -> Vec<&str> {
        o.dispatches.iter().map(|d| d.step.id.as_str()).collect()
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn status_of(sess: &Session, plan: &str, step: &str) -> Option<TaskStatus> {
        sess.tasks()
            .into_iter()
            .find(|t| t.plan_id == plan && t.step_id == step)
            .map(|t| t.status)
    }

    #[test]
    fn first_run_executes_safe_steps_and_parks_the_rest() {
        let mut sess = Session::new();
        sess.add_plan(mixed());
        let out = sess.begin_run("p1", &[], 10).unwrap();
        assert_eq!(ids(&out), ["s1"]);
        assert_eq!(status_of(&sess, "p1", "s1"), Some(TaskStatus::Running));
        assert_eq!(
            status_of(&sess, "p1", "s2"),
            Some(TaskStatus::AwaitingConfirmation)
        );
        assert_eq!(
            status_of(&sess, "p1", "s4"),
            Some(TaskStatus::AwaitingConfirmation)
        );
        assert_eq!(
            status_of(&sess, "p1", "s3"),
            None,
            "a blocked step never becomes a task"
        );
        assert_eq!(out.touched.len(), 3);
    }

    #[test]
    fn confirming_runs_exactly_that_step() {
        let mut sess = Session::new();
        sess.add_plan(mixed());
        sess.begin_run("p1", &[], 10).unwrap();
        let out = sess.begin_run("p1", &s(&["s2"]), 20).unwrap();
        assert_eq!(ids(&out), ["s2"]);
        assert_eq!(status_of(&sess, "p1", "s2"), Some(TaskStatus::Running));
        assert_eq!(
            status_of(&sess, "p1", "s4"),
            Some(TaskStatus::AwaitingConfirmation)
        );
        // s1 was not run a second time
        assert_eq!(out.touched.len(), 1);
    }

    #[test]
    fn nothing_runs_twice() {
        let mut sess = Session::new();
        sess.add_plan(mixed());
        let first = sess.begin_run("p1", &s(&["s2", "s4"]), 10).unwrap();
        assert_eq!(ids(&first), ["s1", "s2", "s4"], "plan order");
        for _ in 0..3 {
            let again = sess.begin_run("p1", &s(&["s2", "s4", "s1"]), 11).unwrap();
            assert!(again.dispatches.is_empty() && again.touched.is_empty());
        }
        assert_eq!(sess.tasks().len(), 3);
    }

    #[test]
    fn confirming_in_the_first_call_skips_the_waiting_state() {
        let mut sess = Session::new();
        sess.add_plan(mixed());
        let out = sess.begin_run("p1", &s(&["s4"]), 10).unwrap();
        assert_eq!(ids(&out), ["s1", "s4"]);
        assert_eq!(
            status_of(&sess, "p1", "s2"),
            Some(TaskStatus::AwaitingConfirmation)
        );
    }

    #[test]
    fn blocked_steps_never_run_whatever_is_confirmed() {
        let mut sess = Session::new();
        sess.add_plan(mixed());
        let out = sess
            .begin_run("p1", &s(&["s3", "s1", "s2", "s4", "s3"]), 10)
            .unwrap();
        assert!(!ids(&out).contains(&"s3"));
        assert_eq!(status_of(&sess, "p1", "s3"), None);
        let only_blocked = plan("p2", vec![step("s1", Risk::Blocked("no".into()))]);
        sess.add_plan(only_blocked);
        let out = sess.begin_run("p2", &s(&["s1"]), 11).unwrap();
        assert!(out.dispatches.is_empty() && out.touched.is_empty());
    }

    #[test]
    fn confirm_steps_need_their_own_id() {
        let mut sess = Session::new();
        sess.add_plan(mixed());
        // ids that do not exist, other plans' ids, empty strings, case changes: all ignored
        let out = sess
            .begin_run("p1", &s(&["", "S2", "s9", "p1.s2", " s2", "s2 "]), 10)
            .unwrap();
        assert_eq!(ids(&out), ["s1"]);
        assert_eq!(
            status_of(&sess, "p1", "s2"),
            Some(TaskStatus::AwaitingConfirmation)
        );
    }

    #[test]
    fn a_cancelled_step_cannot_be_revived() {
        let mut sess = Session::new();
        sess.add_plan(mixed());
        sess.begin_run("p1", &[], 10).unwrap();
        let waiting = sess
            .tasks()
            .into_iter()
            .find(|t| t.step_id == "s2")
            .unwrap();
        let t = sess.cancel(waiting.id, 12).unwrap();
        assert_eq!(t.status, TaskStatus::Cancelled);
        let out = sess.begin_run("p1", &s(&["s2"]), 13).unwrap();
        assert!(out.dispatches.is_empty());
        assert_eq!(status_of(&sess, "p1", "s2"), Some(TaskStatus::Cancelled));
    }

    #[test]
    fn running_tasks_cannot_be_cancelled() {
        let mut sess = Session::new();
        sess.add_plan(mixed());
        let out = sess.begin_run("p1", &[], 10).unwrap();
        assert!(sess.cancel(out.dispatches[0].task_id, 11).is_err());
    }

    #[test]
    fn finishing_records_done_or_failed() {
        let mut sess = Session::new();
        sess.add_plan(mixed());
        let out = sess.begin_run("p1", &s(&["s2"]), 10).unwrap();
        let (a, b) = (out.dispatches[0].task_id, out.dispatches[1].task_id);
        assert_eq!(sess.finish(a, Ok(()), 11).unwrap().status, TaskStatus::Done);
        assert_eq!(
            sess.finish(b, Err("Windows could not open it".into()), 12)
                .unwrap()
                .status,
            TaskStatus::Failed("Windows could not open it".into())
        );
        assert!(sess.finish(a, Ok(()), 13).is_err(), "already final");
        assert_eq!(sess.task(a).unwrap().updated_at, 11);
    }

    #[test]
    fn unknown_and_forgotten_plans() {
        let mut sess = Session::new();
        assert_eq!(
            sess.begin_run("nope", &[], 0),
            Err(SessionError::UnknownPlan)
        );
        for i in 0..=PLAN_CAP {
            sess.add_plan(plan(&format!("p{i}"), vec![step("s1", Risk::Safe)]));
        }
        assert!(sess.plan("p0").is_none(), "oldest plan forgotten");
        assert!(sess.plan("p1").is_some());
        assert_eq!(sess.begin_run("p0", &[], 0), Err(SessionError::UnknownPlan));
        assert!(SessionError::UnknownPlan.to_string().contains("expired"));
    }

    #[test]
    fn re_adding_a_plan_replaces_it() {
        let mut sess = Session::new();
        sess.add_plan(plan("p", vec![step("s1", Risk::Safe)]));
        sess.add_plan(plan(
            "p",
            vec![step("s1", Risk::Safe), step("s2", Risk::Safe)],
        ));
        assert_eq!(sess.plan("p").unwrap().steps.len(), 2);
    }

    #[test]
    fn a_full_queue_fails_cleanly_without_stranding_running_tasks() {
        let mut sess = Session::new();
        // fill the queue with waiting tasks (one confirm step per plan)
        for i in 0..HISTORY_CAP - 1 {
            let id = format!("w{i}");
            sess.add_plan(plan(&id, vec![step("s1", Risk::Confirm)]));
            sess.begin_run(&id, &[], 1).unwrap();
        }
        // one slot left, but this plan needs two tasks
        sess.add_plan(plan(
            "big",
            vec![step("s1", Risk::Safe), step("s2", Risk::Safe)],
        ));
        let err = sess.begin_run("big", &[], 2).unwrap_err();
        assert_eq!(err, SessionError::Task(TaskError::Full));
        assert!(err.to_string().contains("Cancel some"));
        // the one step that did start is not left "running" without anyone to finish it
        assert!(matches!(
            status_of(&sess, "big", "s1"),
            Some(TaskStatus::Failed(_))
        ));
    }

    #[test]
    fn tasks_carry_the_step_summary() {
        let mut sess = Session::new();
        sess.add_plan(mixed());
        sess.begin_run("p1", &[], 10).unwrap();
        let t = sess
            .tasks()
            .into_iter()
            .find(|t| t.step_id == "s1")
            .unwrap();
        assert_eq!(t.summary, "Step s1");
        assert_eq!(t.created_at, 10);
    }
}
