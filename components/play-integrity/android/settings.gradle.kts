// SPDX-License-Identifier: GPL-3.0-only
// From Play Integrity Fork, revision 23ec56c2440c4fd1a5e893a23ef7861d6366a828.
// Modified 2026-10-23: independent engine packaging.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}
dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "PlayIntegrityFix"
include(":app")
