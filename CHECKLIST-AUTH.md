Was zu ändern ist, damit der Flow funktioniert:

Token an /auth/change-password senden. Optionen:
authRefreshInterceptor so anpassen, dass nur /auth/login, /auth/refresh, /auth/logout, /auth/oidc/\* übersprungen werden, aber nicht /auth/change-password.
Oder im PasswordAuthService.changePassword den Session‑Token explizit per Request‑Option mitsenden.
Für Password‑Login sicherstellen: Croniq:Auth:Oidc:Enabled=false (oder den OIDC‑Only‑Reject abschalten), sonst werden die Tokens nicht akzeptiert.
Wenn du willst, setze ich die Variante 1 oder 2 direkt um.
