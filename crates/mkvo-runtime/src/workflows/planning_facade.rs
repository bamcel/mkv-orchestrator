use mkvo_contracts::LibraryAuditResponse;

use crate::compat::{
    LibraryAuditRequest, MuxPreviewRequest, MuxPreviewResponse, PropEditPreviewRequest,
    PropEditPreviewResponse, PropEditTemplateRequest, PropEditTemplateResponse,
    RenamePreviewRequest, RenamePreviewResponse,
};
use crate::{MkvoRuntime, RuntimeResult};

impl MkvoRuntime {
    pub async fn build_rename_preview(
        &self,
        request: RenamePreviewRequest,
    ) -> RuntimeResult<RenamePreviewResponse> {
        self.build_rename_preview_impl(request).await
    }

    pub async fn build_mux_preview(
        &self,
        request: MuxPreviewRequest,
    ) -> RuntimeResult<MuxPreviewResponse> {
        self.build_mux_preview_impl(request).await
    }

    pub async fn load_propedit_template(
        &self,
        request: PropEditTemplateRequest,
    ) -> RuntimeResult<PropEditTemplateResponse> {
        self.load_propedit_template_impl(request).await
    }

    pub async fn build_propedit_preview(
        &self,
        request: PropEditPreviewRequest,
    ) -> RuntimeResult<PropEditPreviewResponse> {
        self.build_propedit_preview_impl(request).await
    }

    pub async fn run_library_audit(
        &self,
        request: LibraryAuditRequest,
    ) -> RuntimeResult<LibraryAuditResponse> {
        self.run_library_audit_impl(request).await
    }
}
