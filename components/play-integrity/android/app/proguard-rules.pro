# SPDX-License-Identifier: GPL-3.0-only
# From Play Integrity Fork, revision 23ec56c2440c4fd1a5e893a23ef7861d6366a828.
# Modified 2026-10-23: independent engine packaging.
-keep class es.chiteroman.playintegrityfix.EntryPoint { public <methods>; }
-keep class es.chiteroman.playintegrityfix.EntryPointVending { public <methods>; }
-keepclassmembers class es.chiteroman.playintegrityfix.CustomKeyStoreSpi
-keepclassmembers class es.chiteroman.playintegrityfix.CustomPackageInfoCreator { public <init>(...); }
-keepclassmembers class es.chiteroman.playintegrityfix.CustomProvider
