# Licensing boundaries

Duck ToolBox's independently written management code remains under the root MIT
license. The Play Integrity engine is a separate program distributed in the same
installation archive. Its native library is loaded by Zygisk into selected Android
processes; it is not linked into the `duckd` management executable. Management uses
the documented engine CLI and configuration formats.

All code derived from Play Integrity Fork, including translated or adapted code,
is kept in the independently built `components/play-integrity` project under GPLv3,
except dependencies with their own notices. See its LICENSE, NOTICE and UPSTREAM.json.
The root MIT license does not replace those terms.

Binary distributions include `licenses/play-integrity`, the matching engine
corresponding-source archive, dependency notices and build metadata. The feature
footer exposes the upstream project, license and corresponding source. Source
archives include the native dependencies with the actual ShadowHook patch applied.

Existing third-party notices, including `module/licenses/rkp-v1-oracle.txt`, remain
applicable to their respective components.
