// Maven publishing config for the published modules. Applied by core,
// spring-boot-starter, kotlin-ext, and otel — NOT conformance-tests (test-only).
//
// Coordinates / POM metadata / source+javadoc attachments are set up here.
// Signing (GPG) is enabled only when GPG_SIGNING_KEY is present in the
// environment, so local `publishToMavenLocal` runs need no signing setup.
// Sonatype OSSRH staging is wired in the root build via the
// gradle-nexus-publish-plugin.

plugins {
    `maven-publish`
    signing
}

publishing {
    publications {
        register<MavenPublication>("maven") {
            from(components["java"])

            pom {
                name.set("${project.group}:${project.name}")
                description.set(provider { project.description ?: project.name })
                url.set("https://github.com/nuetzliches/croniq")

                // Dual licence to match the repo root LICENSE-MIT /
                // LICENSE-APACHE files. Maven Central accepts either
                // individually; we list both so downstream tooling
                // (SBOM generators, OSV scanners) sees the full picture.
                licenses {
                    license {
                        name.set("MIT")
                        url.set("https://opensource.org/licenses/MIT")
                    }
                    license {
                        name.set("Apache-2.0")
                        url.set("https://www.apache.org/licenses/LICENSE-2.0")
                    }
                }

                developers {
                    developer {
                        id.set("croniq")
                        name.set("Croniq contributors")
                        url.set("https://github.com/nuetzliches/croniq/graphs/contributors")
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
    }
}

// GPG signing — only active when the env vars are populated (CI release
// jobs supply them from GitHub Actions secrets). Local development and
// publishToMavenLocal don't sign, so contributors don't need their own
// keys to verify changes.
signing {
    val signingKey: String? = providers.environmentVariable("GPG_SIGNING_KEY").orNull
    val signingPassword: String? = providers.environmentVariable("GPG_SIGNING_PASSWORD").orNull
    if (!signingKey.isNullOrBlank() && signingPassword != null) {
        useInMemoryPgpKeys(signingKey, signingPassword)
        sign(publishing.publications["maven"])
    }
}

tasks.withType<Sign>().configureEach {
    onlyIf { !providers.environmentVariable("GPG_SIGNING_KEY").orNull.isNullOrBlank() }
}
