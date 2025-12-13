import { isDevMode } from '@angular/core';

const PROD_BASE_URL = 'https://api.croniq.dev';
const DEV_BASE_URL = 'http://localhost:5000';

const envBaseUrl = import.meta.env?.NG_APP_API_BASE_URL?.trim();
const resolvedBaseUrl = envBaseUrl && envBaseUrl.length > 0 ? envBaseUrl : isDevMode() ? DEV_BASE_URL : PROD_BASE_URL;

const envSwaggerUiUrl = import.meta.env?.NG_APP_SWAGGER_UI_URL?.trim();
const resolvedSwaggerUiUrl = envSwaggerUiUrl && envSwaggerUiUrl.length > 0
    ? envSwaggerUiUrl
    : new URL('/swagger/index.html', resolvedBaseUrl).toString();

export const API_CONFIG = {
    baseUrl: resolvedBaseUrl,
    swaggerUiUrl: resolvedSwaggerUiUrl,
} as const;
