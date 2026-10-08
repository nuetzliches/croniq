//! Putting a page of the notes inbox together, shared by both store backends.
//!
//! Each backend reads a page in three steps, in its own dialect: the threads,
//! then the newest notes of each, then the runs they name. Handing the notes
//! and runs to their threads is the same in both, and lives here.

use std::collections::HashMap;

use uuid::Uuid;

use crate::models::{Execution, JobNote, NoteThread};

/// What a page's notes and runs are looked up by.
pub(crate) struct ThreadKeys {
    /// The runs of the page's run threads.
    pub run_ids: Vec<Uuid>,
    /// The jobs of the page's job threads — their notes name no run.
    pub job_keys: Vec<String>,
}

pub(crate) fn thread_keys(threads: &[NoteThread]) -> ThreadKeys {
    let mut run_ids = Vec::new();
    let mut job_keys = Vec::new();
    for thread in threads {
        match thread.execution_id {
            Some(id) => run_ids.push(id),
            None => job_keys.push(thread.job_key.clone()),
        }
    }
    ThreadKeys { run_ids, job_keys }
}

/// Hand each thread its notes and its run.
///
/// `notes` arrive newest first, and each thread keeps that order. A note is
/// matched on job *and* run, as the threads were grouped, so a job's own
/// thread never picks up the notes of its runs.
pub(crate) fn fill(threads: &mut [NoteThread], notes: Vec<JobNote>, executions: Vec<Execution>) {
    let index: HashMap<(String, Option<Uuid>), usize> = threads
        .iter()
        .enumerate()
        .map(|(i, thread)| ((thread.job_key.clone(), thread.execution_id), i))
        .collect();
    for note in notes {
        if let Some(&i) = index.get(&(note.job_key.clone(), note.execution_id)) {
            threads[i].notes.push(note);
        }
    }

    let runs: HashMap<Uuid, Execution> = executions.into_iter().map(|e| (e.id, e)).collect();
    for thread in threads.iter_mut() {
        if let Some(id) = thread.execution_id {
            thread.execution = runs.get(&id).cloned();
        }
    }
}
