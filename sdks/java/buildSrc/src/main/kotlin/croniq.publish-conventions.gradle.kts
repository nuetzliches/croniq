// Maven publishing config for the published modules. Applied by core,
// spring-boot-starter, and kotlin-ext — NOT conformance-tests (test-only).
//
// PR-1 wires the Maven coordinates, POM metadata, and source/javadoc
// attachments so `publishToMavenLocal` produces a release-quality artefact.
// Sonatype OSSRH / Maven Central staging credentials are added in PR-7.

plugins {
    `maven-publish`
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
