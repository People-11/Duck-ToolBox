# Device ID provisioning backends

The `device-ids` feature uses contract 3. `defaults` returns the existing profile fields
plus the detected `backend`. `provision --stdin-json` accepts the same profile input.
Qualcomm QSEECom and MediaTek TrustKernel use the shared property/kernel SoC detector;
conflicting evidence is rejected. Unknown vendor information retains the historical
QSEECom fallback, whose library loader still determines whether the API is available.

## Qualcomm

The QSEECom command framing, field order, TA version negotiation and success marker are
unchanged. Serial remains required. TA name/path overrides apply to this backend.
Successful results identify the loaded library and use `verification=ta_acknowledgement`.

## TrustKernel

MediaTek defaults use `keymaster` at `/vendor/app/t6` for compatibility with existing
profiles. KPH addresses the following targets by UUID, independently of that QSEECom-style
TA location. Custom TA location overrides are rejected rather than silently ignored.

| Target | UUID | Payload |
| --- | --- | --- |
| `rkp_flag` | `b46325e6-5c90-8252-2ead-a8e32e5180d6` | `BRAND=... MODEL=... PLATFORM=... PRODUCT=...` |
| `attestation_ids` | `2ea702fa-17bc-4752-b3ad-b2871a772347` | `BRAND=... DEVICE=... PRODUCT=... MANUFACTURER=... MODEL=... [SERIAL=...] [IMEI=...] [IMEI2=...]` |

Values use the normalized selected identifiers; `PLATFORM` uses the same property
snapshot. Serial and the two IMEIs are optional on this path. MEID/MEID2 are not part of
the known ASCII payload contract: they remain supported by QSEECom, and appear in
`omitted_ids` rather than the written ID count for TrustKernel. Control characters,
ambiguous `=` tokens and field values exceeding 1024 bytes are rejected.

Each payload contains a trailing NUL byte. Dry runs serialize the same prepared
requests used by live execution, including exact payload bytes and target UUIDs.
The adapter calls `kph_import_ta_data_by_uuid(uuid, tag, context, context_len,
payload, payload_len, storage)`: a seven-argument ABI. The context is 128 zeroed
bytes with permission 42 stored in the first little-endian u32; storage is 0.
Payload length includes the trailing NUL.

Both targets are imported before `kph_verify_data_by_uuid(uuid, tag)` checks their
existence. Successful results say `verification=existence_only` and identify the actual
`loaded_library`; `executable`, `ta_name`, `ta_path` and `ta_api_version` are null.
Dry runs also leave `loaded_library` null. Contract 3 replaces contract 2's external
executable metadata with runtime library metadata. Existence does not prove that the
stored bytes equal the request.

## Library and process ownership

No KPH executable is bundled or executed; vendor libraries come from the device. Live writes require
Android arm64, `/dev/tkcoredrv` and the device's `libkphproxy.so`, searched under
`/vendor/lib64`, `/odm/lib64`, their `hw` subdirectories, then `/system/lib64`.
Its dependent libraries must also be available to the Android loader. Missing required
UUID import or verification exports fail before the first import; legacy numeric-ID
exports are never substituted. `kph_ta_version(buffer, 32)` is optional.

The parent starts the same installed backend as a disposable worker for each vendor
call. Only the worker loads vendor code and receives the vendor library search path.
A file lock serializes TrustKernel operations. Request directories are mode 0700;
request and reply files are mode 0600, size bounded, and read without following
symlinks. A previous reply is removed before each invocation. A vendor crash or a
missing/malformed reply cannot count as a successful import.

Every worker has a 30-second timeout and ownership that kills/reaps it on normal
error/timeout unwinding. Vendor stdout/stderr cannot enter the command's JSON stream
or public logs. Diagnostics are bounded to 8192 bytes per stream/invocation and retained
in the private report. Temporary files are removed on success and error. Process
isolation contains crashes and hangs; the worker still needs root privileges for the
device driver and is not a security sandbox.

## Failures and reports

A private report is published before live execution and updated when the operation
returns, including on failure. The error envelope carries `report_path`, `backend` and
per-target `writes`. A write record distinguishes attempted import, successful command
return and existence-check result. A timeout or nonzero import result does not prove
that the TA left storage unchanged. The two writes are not atomic; there is no automatic
rollback or destructive remove-and-retry behavior.

Reports and their directories use modes 0600 and 0700. A report publication failure after
execution is returned with the same write details rather than triggering another write.

## Evidence and limits

The UUID import/verification signatures, 128-byte context initialization and TA version
call are based on the original PR #35 KPH sample, SHA-256
`a8112084eccbf91f55368aca69c4cb656ed0a88859b0bdc66fcd8845dcc9f507`, inspected with
Ghidra. The original sample and its patch are no longer distributed. This observation
does not establish ABI compatibility with every `libkphproxy.so` version. In particular,
`kph_import_ta_data` and `kph_verify_ta_data2` use a different addressing contract and
must not be called with UUID pointers under guessed signatures.

Host tests use a synthetic C shared library to verify argument order, buffer lengths,
context initialization, target selection and raw return status, without loading vendor
code. Process tests cover sequencing, partial failure, stale replies, timeout, privacy
and cleanup. Successful provisioning and attestation on real MediaTek hardware still
require device validation. This backend writes device IDs; device RKP issuance remains
a separate V1 AIDL feature on MediaTek.
