interface ImportMetaEnv {
    readonly NG_APP_API_BASE_URL?: string;
    readonly NG_APP_SWAGGER_UI_URL?: string;
}

interface ImportMeta {
    readonly env: ImportMetaEnv;
}
