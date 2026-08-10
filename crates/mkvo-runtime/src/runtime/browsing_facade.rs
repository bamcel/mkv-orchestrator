use mkvo_contracts::FileSystemResponse;

use super::MkvoRuntime;
use crate::RuntimeResult;

impl MkvoRuntime {
    /// Browse through the host policy boundary without exposing its internal
    /// authorization and UNC/volume navigation implementation.
    pub async fn browse_file_system(
        &self,
        path: Option<String>,
    ) -> RuntimeResult<FileSystemResponse> {
        self.browse_file_system_impl(path).await
    }
}
