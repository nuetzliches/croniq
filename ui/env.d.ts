/// <reference types="vite/client" />

interface ImportMetaEnv {
  /**
   * The release this bundle was built for, stamped from a Docker `ARG` in the
   * `ui-builder` stage. Absent on every build that is not a release — see
   * `app/lib/build-version.ts`, which treats absent as "say nothing".
   */
  readonly VITE_APP_VERSION?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
