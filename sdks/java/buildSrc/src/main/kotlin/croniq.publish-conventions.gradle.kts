// Maven publishing config for the published modules. Applied by core,
// spring-boot-starter, and kotlin-ext — NOT conformance-tests (test-only).
//
// Uses the Vanniktech maven-publish plugin which wraps the Sonatype Central
// Portal API: bundle creation, GPG signing, upload, and auto-release in one
// plugin. Credentials come from env vars on the release runner; PR builds
// run with empty credentials and the plugin skips the publish-to-Central
// path while still exercising publishToMavenLocal.

import com.vanniktech.maven.publish.SonatypeHost

plugins {
    id("com.vanniktech.maven.publish")
}

mavenPublishing {
    // Stage to the NEW Central Portal (https://central.sonatype.com) and
    // auto-release after the upload succeeds — no manual click in the
    // portal UI. The release workflow's verify steps already gate the
    // publish, so the manual stage isn't adding signal.
    //
    // SonatypeHost.CENTRAL_PORTAL is mandatory: the Vanniktech 0.30
    // default is SonatypeHost.DEFAULT which targets the LEGACY OSSRH UI
    // (s01.oss.sonatype.org) via the stagingProfiles endpoint. Namespaces
    // verified after 2024-06 — including io.github.nuetzliches — exist
    // only on the new portal, and OSSRH responds with HTTP 402 to such
    // accounts ("Cannot get stagingProfiles for account ...: 402").
    publishToMavenCentral(host = SonatypeHost.CENTRAL_PORTAL, automaticRelease = true)

    // Maven Central rejects unsigned artefacts. The plugin reads the
    // ASCII-armored key + passphrase from ORG_GRADLE_PROJECT_signingInMemoryKey
    // and ORG_GRADLE_PROJECT_signingInMemoryKeyPassword. If the key is
    // unset (PR builds), signing is silently skipped — so the publishToMavenLocal
    // smoke check still passes without GPG secrets on PR runners.
    signAllPublications()

    // Per-module coordinates are set in each subproject's build.gradle.kts
    // via `mavenPublishing { coordinates(group, "<artifact-id>", version) }`.
    // The default artifactId would be the project name (e.g. "core"), which
    // is ambiguous under the io.github.nuetzliches namespace — we use the
    // explicit `croniq-runner*` prefix to disambiguate.

    pom {
        // Vanniktech defaults pom.name to `${groupId}:${artifactId}` if not
        // set explicitly here. Each subproject sets its own description via
        // `description = "..."` in build.gradle.kts; that flows through to
        // pom.description automatically.
        description.set(provider { project.description ?: project.name })
        url.set("https://github.com/nuetzliches/croniq")
        inceptionYear.set("2026")

        // Dual licence to match the repo root LICENSE-MIT / LICENSE-APACHE
        // files. Maven Central accepts either individually; we list both
        // so downstream tooling (SBOM generators, OSV scanners) sees the
        // full picture.
        licenses {
            license {
                name.set("MIT")
                url.set("https://opensource.org/licenses/MIT")
                distribution.set("repo")
            }
            license {
                name.set("Apache-2.0")
                url.set("https://www.apache.org/licenses/LICENSE-2.0")
                distribution.set("repo")
            }
        }

        developers {
            developer {
                id.set("nuetzliches")
                name.set("Sebastian Gieseler")
                url.set("https://github.com/nuetzliches")
            }
        }

        scm {
            connection.set("scm:git:https://github.com/nuetzliches/croniq.git")
            developerConnection.set("scm:git:ssh://git@github.com/nuetzliches/croniq.git")
            url.set("https://github.com/nuetzliches/croniq")
        }

        issueManagement {
            system.set("GitHub")
            url.set("https://github.com/nuetzliches/croniq/issues")
        }
    }
}
