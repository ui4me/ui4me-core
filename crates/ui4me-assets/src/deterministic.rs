//! owner: 007-resources (T024) — deterministic resource mode (FR-067, R-09).
//!
//! No pool exists. Pending jobs (reads, decodes, font validation) are queued here and run
//! synchronously on the owner thread when `process_completions` (or a drain point) processes
//! them; their completions are posted to the same queue and form that step's batch. There is no
//! cross-thread wait anywhere. Only *where* a job runs differs from the asynchronous mode: the
//! request state machine, the limits and the policy are shared.

use crate::worker::{CompletionQueue, Job, JobContext, run_job};
use std::collections::VecDeque;

#[derive(Debug, Default)]
pub struct JobQueue {
    jobs: VecDeque<Job>,
}

impl JobQueue {
    pub fn push(&mut self, job: Job) -> bool {
        self.jobs.retain(|pending| !pending.cancel.is_canceled());
        if self.jobs.len() >= crate::worker::MAX_ADMITTED_JOBS {
            return false;
        }
        let source_bytes: usize = self
            .jobs
            .iter()
            .map(|pending| pending.url.as_str().len() + 256)
            .sum();
        if source_bytes.saturating_add(job.url.as_str().len() + 256) as u64
            > crate::worker::MAX_QUEUED_SOURCE_BYTES
        {
            return false;
        }
        self.jobs.push_back(job);
        true
    }

    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    pub fn clear(&mut self) {
        self.jobs.clear();
    }

    /// Run every queued job to a completion, in submission order. A job canceled before it runs
    /// produces no completion (as a worker would skip it).
    pub fn run_all(&mut self, ctx: &JobContext, completions: &CompletionQueue) {
        while completions.has_capacity() {
            let Some(job) = self.jobs.pop_front() else {
                break;
            };
            if job.cancel.is_canceled() {
                continue;
            }
            let _ = completions.post(run_job(&job, ctx));
        }
    }
}
