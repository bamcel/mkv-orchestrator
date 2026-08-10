use mkvo_contracts::{JobEventEnvelope, JobKind, LogQuery};
use tokio::sync::broadcast;

use super::{LogExport, MkvoRuntime, RecentJobsResponse, parse_job_id};
use crate::compat::{OperationJobResponse, OperationLogResponse};
use crate::{RuntimeError, RuntimeResult};

impl MkvoRuntime {
    pub async fn get_operation_job(&self, id: &str) -> RuntimeResult<OperationJobResponse> {
        let id = parse_job_id(id)?;
        let snapshot = self
            .inner
            .jobs
            .get(id)
            .await?
            .ok_or_else(|| RuntimeError::not_found(format!("operation job {id}")))?;
        if snapshot.kind == JobKind::Scan {
            return Err(RuntimeError::not_found(format!("operation job {id}")));
        }
        Ok(OperationJobResponse::from_snapshot(&snapshot))
    }

    pub async fn cancel_operation_job(&self, id: &str) -> RuntimeResult<OperationJobResponse> {
        let id = parse_job_id(id)?;
        let snapshot = self.inner.jobs.cancel(id).await?;
        if snapshot.kind == JobKind::Scan {
            return Err(RuntimeError::not_found(format!("operation job {id}")));
        }
        Ok(OperationJobResponse::from_snapshot(&snapshot))
    }

    pub async fn subscribe_job_events(
        &self,
        id: &str,
    ) -> RuntimeResult<broadcast::Receiver<JobEventEnvelope>> {
        self.inner
            .jobs
            .subscribe(parse_job_id(id)?)
            .await
            .map_err(Into::into)
    }

    pub async fn get_logs(&self) -> RuntimeResult<OperationLogResponse> {
        Ok(OperationLogResponse {
            entries: self
                .inner
                .dependencies
                .logs
                .query(&LogQuery::default())
                .await?,
        })
    }

    pub async fn clear_logs(&self) -> RuntimeResult<OperationLogResponse> {
        self.inner.dependencies.logs.clear().await?;
        self.get_logs().await
    }

    pub async fn export_logs(&self) -> RuntimeResult<LogExport> {
        self.export_logs_impl().await
    }

    pub async fn list_recent_jobs(
        &self,
        limit: Option<usize>,
    ) -> RuntimeResult<RecentJobsResponse> {
        self.list_recent_jobs_impl(limit).await
    }
}
