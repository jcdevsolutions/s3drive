//! Job execution: spawn rclone, stream its log to the UI, and record the outcome.
//!
//! S3Dive never runs rclone through a shell; arguments come from the tested core
//! builders and the child is killed by pid when the user stops a run.

use crate::context::{AppContext, RunEntry};
use crate::runner::Rclone;
use s3dive_core::model::{now_ms, Job, RunOutcome, RunSummary};
use s3dive_core::rclone::{self, TransferOptions};
use s3dive_core::validate;
use serde::Serialize;
use std::process::Stdio;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobLine {
    pub run_id: String,
    pub job_id: String,
    pub stream: String,
    pub line: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobFinished {
    pub job_id: String,
    pub summary: RunSummary,
}

fn pump<R>(reader: Option<R>, is_stderr: bool, sink: UnboundedSender<(bool, String)>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let Some(reader) = reader else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if sink.send((is_stderr, line)).is_err() {
                break;
            }
        }
    });
}

/// Validate the job and build its argument vector. Does not reserve anything.
fn prepare(
    app: &AppHandle,
    job_id: &str,
    resync: bool,
) -> Result<(Job, Rclone, Vec<String>), String> {
    let context = app.state::<AppContext>();
    let state = context.state();
    let job = state
        .find_job(job_id)
        .cloned()
        .ok_or_else(|| format!("Job '{job_id}' no longer exists."))?;
    validate::validate_job(&state, &job)?;

    let rclone = context.rclone();
    rclone.program()?;
    let args = rclone::transfer_args(
        &job,
        &state.settings,
        &context.paths.conf_arg(),
        TransferOptions { resync },
    );
    Ok((job, rclone, args))
}

/// Spawn rclone, then persist the running summary. If persisting fails the child is
/// killed immediately, so a failed write never leaves an untracked rclone running or a
/// job stuck on "running" forever.
fn persist_and_spawn(
    app: &AppHandle,
    program: &std::path::Path,
    args: &[String],
    job_id: &str,
    summary: &RunSummary,
) -> Result<Child, String> {
    let child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Cannot start rclone: {error}"))?;

    let context = app.state::<AppContext>();
    let persisted = context.mutate(|state| {
        if let Some(stored) = state
            .jobs
            .iter_mut()
            .find(|candidate| candidate.id == job_id)
        {
            stored.last_run = Some(summary.clone());
        }
        Ok(())
    });

    if let Err(error) = persisted {
        if let Some(pid) = child.id() {
            crate::runner::kill_process(pid);
        }
        return Err(error);
    }

    Ok(child)
}

/// Release every reservation for a run that never made it to the supervisor.
fn abandon(app: &AppHandle, run_id: &str, job_id: &str) {
    let context = app.state::<AppContext>();
    context
        .runs
        .lock()
        .expect("run lock poisoned")
        .remove(run_id);
    context
        .active_jobs
        .lock()
        .expect("run lock poisoned")
        .remove(job_id);
}

/// Start a run and return its immediate summary. Progress arrives as events.
pub async fn run(app: AppHandle, job_id: String, resync: bool) -> Result<RunSummary, String> {
    let (job, rclone, args) = prepare(&app, &job_id, resync)?;
    let program = rclone.program()?.to_path_buf();

    let run_id = s3dive_core::new_id();
    let summary = RunSummary {
        run_id: run_id.clone(),
        started_at_ms: now_ms(),
        finished_at_ms: None,
        outcome: RunOutcome::Running,
        exit_code: None,
        message: None,
    };

    // Claim the job atomically: one run per job, and the claim is taken before the
    // spawn so two clicks can never start two children.
    {
        let context = app.state::<AppContext>();
        let mut active = context.active_jobs.lock().expect("run lock poisoned");
        if !active.insert(job_id.clone()) {
            return Err(format!("'{}' is already running.", job.name));
        }
    }

    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    {
        let context = app.state::<AppContext>();
        context.runs.lock().expect("run lock poisoned").insert(
            run_id.clone(),
            RunEntry {
                job_id: job_id.clone(),
                pid: None,
                cancel: cancel_tx,
            },
        );
    }

    let mut child = match persist_and_spawn(&app, &program, &args, &job_id, &summary) {
        Ok(child) => child,
        Err(error) => {
            abandon(&app, &run_id, &job_id);
            return Err(error);
        }
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let pid = child.id();

    if let Some(pid) = pid {
        let context = app.state::<AppContext>();
        let mut runs = context.runs.lock().expect("run lock poisoned");
        if let Some(entry) = runs.get_mut(&run_id) {
            entry.pid = Some(pid);
        }
    }

    let (sink, mut stream) = tokio::sync::mpsc::unbounded_channel::<(bool, String)>();
    pump(stdout, false, sink.clone());
    pump(stderr, true, sink.clone());
    drop(sink);

    let emit_app = app.clone();
    let emit_run = run_id.clone();
    let emit_job = job_id.clone();
    let forwarder = tauri::async_runtime::spawn(async move {
        while let Some((is_stderr, line)) = stream.recv().await {
            let _ = emit_app.emit(
                "job:line",
                JobLine {
                    run_id: emit_run.clone(),
                    job_id: emit_job.clone(),
                    stream: if is_stderr { "stderr" } else { "stdout" }.to_string(),
                    line,
                },
            );
        }
    });

    let finish_app = app.clone();
    let finish_run = run_id.clone();
    let finish_job = job_id.clone();
    let started_at_ms = summary.started_at_ms;
    let job_name = job.name.clone();

    tauri::async_runtime::spawn(async move {
        // The `wait()` future borrows `child` for the whole `select!`, so the cancel arm
        // signals the process by pid and the handle is only touched again afterwards.
        let outcome = tokio::select! {
            status = child.wait() => {
                match status {
                    Ok(status) if status.success() => (RunOutcome::Success, status.code(), None),
                    Ok(status) => {
                        let code = status.code();
                        let detail = match code {
                            Some(code) => format!("rclone exited with status {code}."),
                            None => "rclone was terminated by a signal.".to_string(),
                        };
                        (RunOutcome::Failed, code, Some(detail))
                    }
                    Err(error) => (RunOutcome::Failed, None, Some(format!("Cannot wait for rclone: {error}"))),
                }
            }
            _ = cancel_rx => {
                if let Some(pid) = pid {
                    crate::runner::kill_process(pid);
                }
                (RunOutcome::Cancelled, None, Some("Stopped by the user.".to_string()))
            }
        };

        if outcome.0 == RunOutcome::Cancelled {
            let _ = child.wait().await;
        }

        let (outcome, exit_code, message) = outcome;

        // Let every buffered log line reach the UI before announcing the end.
        let _ = forwarder.await;

        let finished = RunSummary {
            run_id: finish_run.clone(),
            started_at_ms,
            finished_at_ms: Some(now_ms()),
            outcome,
            exit_code,
            message,
        };

        let context = finish_app.state::<AppContext>();
        context
            .runs
            .lock()
            .expect("run lock poisoned")
            .remove(&finish_run);
        context
            .active_jobs
            .lock()
            .expect("run lock poisoned")
            .remove(&finish_job);

        if let Err(error) = context.mutate(|state| {
            if let Some(stored) = state
                .jobs
                .iter_mut()
                .find(|candidate| candidate.id == finish_job)
            {
                stored.last_run = Some(finished.clone());
            }
            Ok(())
        }) {
            let _ = finish_app.emit(
                "job:line",
                JobLine {
                    run_id: finish_run.clone(),
                    job_id: finish_job.clone(),
                    stream: "stderr".to_string(),
                    line: format!("Could not save the result of '{job_name}': {error}"),
                },
            );
        }

        let _ = finish_app.emit(
            "job:finished",
            JobFinished {
                job_id: finish_job,
                summary: finished,
            },
        );
    });

    Ok(summary)
}

pub fn cancel(app: AppHandle, run_id: &str) -> Result<(), String> {
    let context = app.state::<AppContext>();
    let entry = context
        .runs
        .lock()
        .expect("run lock poisoned")
        .remove(run_id);

    match entry {
        Some(entry) => entry
            .cancel
            .send(())
            .map_err(|_| "That run had already finished.".to_string()),
        None => Err("That run had already finished.".to_string()),
    }
}

/// Best-effort teardown on app exit: a destructive mirror must never keep running
/// after the window is gone. Synchronous because the async runtime is shutting down.
pub fn kill_all(app: &AppHandle) {
    let context = app.state::<AppContext>();
    for pid in context.running_job_pids() {
        crate::runner::kill_process(pid);
    }
    context.runs.lock().expect("run lock poisoned").clear();
    context
        .active_jobs
        .lock()
        .expect("run lock poisoned")
        .clear();
}
