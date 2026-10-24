# Device ID provisioning backends

The `device-ids` feature uses contract 2. `defaults` returns the existing profile fields
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

Each payload file contains a trailing NUL byte. The reviewed executable reads the file's
actual length; `-p` is a permission/context argument, not a substitute for that byte.
The invocation uses `-p 42 -f 0`. Its named-data trampoline clears 128 context bytes and
stores 42 in the first u32. Dry runs serialize the same prepared requests used by live
execution, including the exact payload bytes, target UUIDs and arguments.

Both targets are imported before their existence is checked. A successful `verify_data`
call cannot establish that the stored bytes match the request. Successful results say
`verification=existence_only`, identify the executable separately from a loaded library,
and leave QSEECom TA/library metadata null. No payload equality claim is made.

## Executable and process ownership

The adapter searches the current module root's `kph_patched`, the conventional
`/data/local/tmp/kph_patched`, then stock `/vendor/bin/kph` or `/system/bin/kph`.
Patched candidates must match the reviewed SHA-256 and ELF ABI in `kph-metadata.json`.
A present but invalid patched candidate is an error, not a reason to run a different
executable silently. Stock candidates must be regular executable files, root-owned
on Android and not writable by group/others. Reads reject symlinks and nonregular files.

The selected bytes are copied into a fresh private directory before execution, so a
later replacement of a staging file cannot change the executed snapshot. The snapshot
digest and source path are recorded in the report. The installer and shared packager
also set executable permissions on the bundled sample; packaging checks its hash, size,
ELF ABI and notice.

Live writes require `/dev/tkcoredrv`, `libkphproxy.so` and `libkphhelper.so`. Only the child
receives the vendor library search path. A file lock serializes TrustKernel operations.
Request directories are mode 0700 and payload files 0600. Every KPH invocation has a
30-second timeout and child ownership that kills/reaps on normal error/timeout unwinding.
Vendor stdout/stderr cannot enter the command's JSON stream or public logs. Diagnostics
are bounded to 8192 bytes per stream/invocation and retained in the private report so TA
error codes remain available; only the parsed TA version enters successful API metadata.
Temporary files are removed on success and error.

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

The bundled sample is byte-for-byte identical to the original PR #35 binary, SHA-256
`a8112084eccbf91f55368aca69c4cb656ed0a88859b0bdc66fcd8845dcc9f507`. An identical published
copy, build identifiers and observed patch are recorded in `module/kph-metadata.json`.
The trampoline is in undeclared RX page padding at file offset `0x48a0`, so its runtime
mapping depends on the loader/page configuration. The stock KPH fallback can fail on
firmware needing the patched context initialization.

The original firmware sample, reproducible patch procedure, vendor library behavior and
redistribution license remain unverified; see `module/licenses/kph-NOTICE.txt`. Hash
validation pins these bytes and does not establish safety or firmware compatibility.
Host tests cover command bytes, process lifecycle, both-target sequencing, partial
failures, report privacy and API/UI contracts. Successful provisioning and attestation
on real MediaTek hardware still require device validation. This backend writes device
IDs; device RKP issuance remains a separate V1 AIDL feature on MediaTek.
