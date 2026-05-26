plugins {
    `java-library`
    id("com.vanniktech.maven.publish") version "0.30.0"
}

// Group ID matches the verified Maven Central namespace (io.github.nuetzliches).
// When/if `io.croniq` gets verified later, switch group + Java packages in
// lockstep — the artifact ID `croniq-runner` stays stable across that move.
group = "io.github.nuetzliches"

java {
    toolchain {
        // JDK 21 — virtual threads (Project Loom) are a hard requirement per
        // issue #133. Older JDKs would force a parallel concurrency design.
        languageVersion = JavaLanguageVersion.of(21)
    }
    withSourcesJar()
    withJavadocJar()
}

repositories {
    mavenCentral()
}

dependencies {
    testImplementation(platform("org.junit:junit-bom:5.11.3"))
    testImplementation("org.junit.jupiter:junit-jupiter")
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}

tasks.test {
    useJUnitPlatform()
    testLogging {
        events("passed", "skipped", "failed")
    }
}

tasks.withType<JavaCompile>().configureEach {
    options.encoding = "UTF-8"
    options.release.set(21)
}

tasks.javadoc {
    (options as StandardJavadocDocletOptions).apply {
        encoding = "UTF-8"
        // Empty javadoc is fine for the smoke-test release; once real code
        // lands the lint pass will catch missing tags. For now permit gaps
        // so the publish workflow does not fail on the placeholder class.
        addStringOption("Xdoclint:none", "-quiet")
    }
}

mavenPublishing {
    // Central Portal API (the post-2024 Sonatype flow). `automaticRelease`
    // skips the manual "Release" click in the portal UI — fine for an
    // automated tag-triggered workflow where the verify steps have already
    // gated the publish.
    publishToMavenCentral(automaticRelease = true)

    // Signs the .jar, sources jar, javadoc jar, and POM with the in-memory
    // GPG key supplied via ORG_GRADLE_PROJECT_signingInMemoryKey. Maven
    // Central rejects unsigned artefacts, so this is non-optional.
    signAllPublications()

    coordinates(group.toString(), "croniq-runner", version.toString())

    pom {
        name.set("Croniq Runner SDK")
        description.set("Java/Kotlin runner SDK for Croniq — polls a Croniq server for work, dispatches handlers, streams structured logs back, reports completion.")
        url.set("https://github.com/nuetzliches/croniq")
        inceptionYear.set("2026")

        licenses {
            license {
                name.set("Apache-2.0")
                url.set("https://www.apache.org/licenses/LICENSE-2.0.txt")
                distribution.set("repo")
            }
            license {
                name.set("MIT")
                url.set("https://opensource.org/licenses/MIT")
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
            url.set("https://github.com/nuetzliches/croniq")
            connection.set("scm:git:git://github.com/nuetzliches/croniq.git")
            developerConnection.set("scm:git:ssh://git@github.com/nuetzliches/croniq.git")
        }

        issueManagement {
            system.set("GitHub")
            url.set("https://github.com/nuetzliches/croniq/issues")
        }
    }
}
