# Commercial architecture

The product has four process boundaries: an unprivileged Tauri UI; a privileged Rust supervisor; the existing self-contained Python/ONNX engine; and a remote control plane that stores device, entitlement, policy assignment, and audit metadata only. Images, crops, URLs, page contents, tensors, and classification events never leave the device.

All downstream work is under `CrossPlatformImageFilter/product/` except the narrow signed-policy bridge in the existing Python package and the root documentation links. The iOS project and its MobileCLIP runtime fix remain upstream-owned and unchanged.

The supervisor owns service lifecycle, exact proxy backup/restore, CA identity, engine and capture health, signed policy activation, license state, secure storage, authorization, recovery, uninstall authorization, and update coordination. The UI can represent only the operations in `supervisor-ipc`; arbitrary commands and localhost administration are absent.
