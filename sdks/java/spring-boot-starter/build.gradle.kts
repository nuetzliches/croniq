// io.croniq:runner-spring-boot-starter — opt-in Spring Boot integration.
// Auto-config + @ConfigurationProperties binding + @CroniqJob registration.
// Adds Spring as a dependency; consumers who don't use Spring pull only
// :core and avoid this artifact entirely.

plugins {
  id("croniq.java-conventions")
  id("croniq.publish-conventions")
}

description = "Spring Boot starter for the Croniq Runner SDK."

// Override the default artifact id (the project name `spring-boot-starter`
// would publish as `io.croniq:spring-boot-starter`, which is ambiguous).
publishing {
  publications.named<MavenPublication>("maven") {
    artifactId = "runner-spring-boot-starter"
  }
}

dependencies {
  api(project(":core"))
  api(libs.spring.boot.starter)
  implementation(libs.spring.boot.autoconfigure)

  // Generates META-INF/spring-configuration-metadata.json so IDEs
  // autocomplete `croniq.runner.*` keys in application.yml.
  annotationProcessor(libs.spring.boot.configuration.processor)

  testImplementation(platform(libs.junit.bom))
  testImplementation(libs.bundles.junit)
  testImplementation(libs.spring.boot.starter.test)
}
