use crate::compat::{MuxPreviewRequest, OperationJobResponse, PropEditPreviewRequest};
use crate::{MkvoRuntime, RuntimeResult};

impl MkvoRuntime {
    pub async fn start_mux_apply(
        &self,
        request: MuxPreviewRequest,
    ) -> RuntimeResult<OperationJobResponse> {
        self.start_mux_apply_impl(request).await
    }

    pub async fn start_propedit_apply(
        &self,
        request: PropEditPreviewRequest,
    ) -> RuntimeResult<OperationJobResponse> {
        self.start_propedit_apply_impl(request).await
    }
}
