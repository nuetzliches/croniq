# Abwägung: UI-Container-Split + Vue-Neubau

Stand: 2026-09-09. Basis: Ist-Analyse von `ui/`, `Dockerfile`, `crates/croniq-server`,
`docs/operations.md`, `docs/vue-migration-plan.md` (2026-06-11) sowie den Referenz-
Projekten `nuts-customer-portal/src/CustomerPortal.App` und `ciphr/ui`.

Es sind **zwei unabhängige Entscheidungen**. Sie lassen sich in beliebiger Reihenfolge
treffen — aber die Reihenfolge *Split zuerst, Framework danach* ist deutlich billiger,
weil der Split den Cutover eines Framework-Wechsels auf "Image-Tag umlegen" reduziert.

---

## Ist-Stand (gemessen)

| Kennzahl | Wert |
|---|---|
| `ui/src` | 93 Dateien, 16.364 LOC (14.076 TS/TSX + 2.288 CSS) |
| Seiten | 10 + 3 Settings-Tabs |
| Größte Brocken | `JobsPage.tsx` 1.478, `LoginPage.tsx` 1.096, `hooks.ts` 832, `AlertsPage.tsx` 785 |
| UI-Tests | 0 Komponenten-/E2E-Tests (nur 5 Unit-Test-Dateien auf `lib`/`api`/`auth`-Hilfsfunktionen) |
| UI-Churn | 35 Commits in 90 Tagen ≈ 2,7/Woche (Juni-Plan nahm 6/Woche an) |
| Auslieferung | Ein Image; `croniq-server --ui-dir /usr/share/croniq/ui` (`ServeDir` + index.html-Fallback) |
| Release-Binaries | enthalten **keine** UI — das Dashboard gibt es heute faktisch nur via Docker |
| Kopplung an Rust | `croniq-config-wasm` (DSL-Preview) wird per `wasm-pack` gebaut und nach `ui/src/lib/wasm/` kopiert |

Seit dem Juni-Plan ist die UI um ~42 % gewachsen (9.900 → 14.076 LOC TSX). Der dort
genannte Aufwand ist entsprechend nicht mehr gültig.

---

## Entscheidung A — UI aus dem Core-Image herauslösen

### Pro

- **Entkoppelte Release-Kadenz.** Ein UI-Fix erzwingt heute den kompletten Multi-Stage-Build
  (Rust-Release je Plattform, wasm-pack, npm) und ein neues Server-Image. Getrennt: ein
  kleines Node-nach-nginx-Image, Sekunden statt Minuten, und der Server behält seinen Digest.
- **Kleinere Supply-Chain im Server-Image-Build.** Der npm-Baum ist heute Teil des Builds,
  der auch das Server-Binary produziert. Getrennte Images = getrennte Build-Kontexte
  (`ciphr/ui` fährt das explizit so, inkl. `npm ci --ignore-scripts` + Lizenz-Gate).
- **Echte Static-Serving-Qualität.** `ServeDir` setzt heute **kein `Cache-Control`** und es gibt
  **keine Kompression** — jeder Dashboard-Load revalidiert jedes Asset. nginx/Caddy liefern
  `immutable` für `/assets/*`, `no-cache` für `index.html`, gzip/brotli und einen eigenen,
  strengeren CSP für den statischen Teil ohne Aufwand mit.
- **Passt zum künftigen Betrieb.** Beide Referenzen fahren genau dieses Muster, und nuts-infra
  hat für croniq noch keinen Stack — der Split lässt sich also einführen, *bevor* ein
  Deployment-Vertrag existiert, den man später brechen müsste.
- **Skalierung/CDN** wird überhaupt erst möglich.

### Contra

- **Same-Origin ist ein harter Vertrag, kein Detail.** Seit #454 liegt der Refresh-Token in
  einem `HttpOnly; SameSite=Strict; Path=/v1/auth`-Cookie. Ein UI auf anderer Origin kann den
  Cookie nicht bekommen — `ui/vite.config.ts` verweigert deshalb absichtlich den Build, wenn
  `VITE_API_URL` ohne `VITE_ALLOW_LOCALSTORAGE_REFRESH=1` gesetzt ist. Ein Split **ohne**
  vorgelagerten Reverse Proxy ist damit eine bewusste Sicherheits-Regression.
  *Auflösbar:* Proxy davor (`/` → UI, `/v1` → Server) — exakt das Muster von `ciphr/nginx.conf`
  ("kein `proxy_pass` im UI-Container, der Proxy davor macht die Origin") und
  `CustomerPortal.App/Caddyfile`. Dann bleibt alles wie es ist.
- **Der Quickstart wird teurer.** `docker compose up` → ein Server + Demo-Runner, Browser auf
  `:4000`. Nach dem Split: UI-Container + Proxy zusätzlich. Für ein selbst-gehostetes
  OSS-Produkt ist die Ein-Container-Story ein Adoptionsargument, das man nicht verschenken sollte.
- **Versions-Skew wird real.** Heute ist UI/API atomar. Getrennt kann UI 0.39 gegen Server 0.38
  laufen. Braucht mindestens: gleicher Tag für beide Images als dokumentierte Regel, plus ein
  Startup-Check gegen `GET /version`.
- **Zwei Auslieferungswege statt einem.** `--ui-dir` muss erhalten bleiben (Air-Gapped,
  Binary-Installs, Einzel-VM), also pflegt man den Pfad trotzdem weiter.
- **Die WASM-Bridge bleibt am Rust-Workspace.** Ein UI-Image braucht weiterhin `wasm-pack` +
  `crates/croniq-config-wasm`. Ein separates **Repo** wäre deshalb teuer; ein separates
  **Image aus demselben Repo** kostet fast nichts.
- Der Caching-Vorteil ist teilweise auch ohne Split zu haben (`SetResponseHeaderLayer` +
  `CompressionLayer` vor `ServeDir`, ~30 Zeilen).

### Empfehlung A: **Ja — additiv, nicht als Ersatz.**

1. Zweites Image `croniq-ui` als **weiteres Target im bestehenden Dockerfile**
   (`FROM nginxinc/nginx-unprivileged AS ui-runtime`), gebaut aus derselben `ui-builder`-Stage.
   Kosten: eine Stage + ein CI-Matrix-Eintrag. Kein zweites Repo.
2. **Muster `ciphr/ui` übernehmen:** der UI-Container proxied *nichts*, er liefert nur Dateien.
   Die Origin macht der Reverse Proxy davor. Damit bleibt der `HttpOnly`-Refresh-Cookie intakt
   und `connect-src 'self'` gültig.
3. **`--ui-dir` und das kombinierte Image bleiben der Default** für Quickstart/Demo/Single-VM.
   Der Split ist die Deployment-Option für nuts-infra & Co., nicht der neue Zwang.
4. Sofort und unabhängig davon: `Cache-Control` + Kompression für `ServeDir` nachrüsten —
   das ist heute schlicht eine Lücke im Auslieferungspfad und betrifft jeden bestehenden Betreiber.
5. Regel dokumentieren: **beide Images tragen denselben Tag**; die UI prüft beim Start
   `/version` und zeigt einen Skew-Hinweis, statt still zu divergieren.

---

## Entscheidung B — Neubau mit Vue 3

### Pro

- **Stack-Konsolidierung.** `nuts-customer-portal` und `ciphr/ui` sind Vue; croniq ist der
  Ausreißer. Für einen Ein-Personen-Betrieb sind die Kontextwechselkosten real und dauerhaft.
- **Konkrete React-Schmerzpunkte verschwinden**, nicht nur abstrakt: der
  `HTMLInputElement`-Prototype-Setter-Hack im `TimezoneInput` (nur für react-hook-form),
  das "Hook liefert JSX"-Confirm-Muster über 8+ Aufrufstellen, `useEffect`-cancelled-Flags rund
  um die WASM-Preview. v-model/Teleport/`watch` ersetzen das ersatzlos.
- **Gelegenheit, aufgelaufene Schuld zu räumen:** `recharts` steht weiterhin in `package.json`
  *und* in den Chunk-Regeln, hat aber **null Importe**; `components.css` definiert `.grid`,
  `.gap-*`, `.grow` unlayered und überschreibt damit Tailwind-Utilities; zwei handgebaute
  SSE-Clients (`hooks.ts`, `ConsolePage.tsx`); keine UI-Tests.
- Etwas kleinere Runtime (Vue 3 vs. React 19 + Radix) — nett, aber kein Argument.

### Contra

- **Kosten ohne Endnutzer-Nutzen.** Der Juni-Plan schätzte 40–48 PT bei 9.900 LOC TSX.
  Bei heute 14.076 LOC sind **55–70 PT** realistisch (~10–14 Kalenderwochen nebenher).
  Kein Feature, kein Bugfix, kein Anwender merkt es.
- **Das Sicherheitsnetz fehlt komplett.** Ohne UI-Tests ist ein 14k-LOC-Framework-Wechsel
  Blindflug; eine Playwright-Smoke-Suite (5–7 PT) ist Vorbedingung. Diese 5–7 PT sind aber
  *unabhängig von Vue* wertvoll — sie fehlen der React-UI heute genauso.
- **Benannte Risikocluster** aus dem Juni-Plan gelten unverändert: vue-query-Reaktivitätsfalle
  (Parameter müssen `computed` sein, sonst frieren Filter ein), ConsolePage-Puffer
  (2.000 Events ⇒ `shallowRef`, sonst Deep-Reactivity pro SSE-Event), Auth-Redirect
  (Vue-Guards feuern nur bei Navigation ⇒ zusätzlicher `watch` nötig), CSS-Selektoren auf
  `data-`-Attributen brechen still.
- **Kein technischer Zwang.** React 19 + Vite 8 + Tailwind 4 ist eine aktuelle, gepflegte Basis.
- **Die beiden Referenzen widersprechen sich** und die Wahl entscheidet die Hälfte des Aufwands:

  | | `nuts-customer-portal` | `ciphr/ui` |
  |---|---|---|
  | Ansatz | Nuxt UI 4 + Pinia + vue-query + valibot + PWA | Vue 3 pur, **null** Runtime-Deps außer `vue` |
  | Eigener Code | wenig, Library trägt das Design | alles selbst, inkl. Router |
  | Für croniq | ersetzt das eigene oklch-Token-System durch Nuxt-UI-Theming ⇒ **Redesign, nicht Port** | für 10 datenreiche Seiten mit Buildern/SSE zu asketisch |

### Empfehlung B: **Jetzt nicht als Big-Bang. Stattdessen dreistufig.**

1. **Sofort (5–8 PT, ohne Vue-Entscheidung wertvoll):** Phase 0 des bestehenden Plans —
   Playwright-Smoke (~12–15 Szenarien), `recharts` + ungenutzte Radix-Deps raus,
   CSS-Kollisionen in `@layer`, SSE-Kern in *ein* getestetes `lib/sse.ts`,
   Token-Holder aus dem Store lösen. Das reduziert die spätere Portierungsfläche
   *und* macht die heutige UI wartbarer. Bei Abbruch: kein Totalverlust.
2. **Danach Entscheidung A umsetzen** (Split). Erst dann ist ein Framework-Cutover trivial
   rollback-bar: neues `croniq-ui`-Image, Proxy-Route umlegen, Rollback = alter Tag.
   Vorher wäre der Cutover ein Dockerfile-COPY-Pfad im Server-Image — machbar, aber
   ohne saubere Rücknahme im laufenden Betrieb.
3. **Vue nur mit schriftlicher strategischer Begründung starten** (der Juni-Plan fordert das
   selbst unter "Vor dem Start zu entscheiden", Punkt 1). Wenn die Begründung
   "eine UI-Familie über nuts/ciphr/croniq" lautet, ist sie legitim — dann aber bitte
   **Port, nicht Redesign**: Reka UI (headless) + die vorhandenen oklch-Tokens, wie im
   Juni-Plan empfohlen. Nuxt UI 4 wäre der Weg, wenn croniq *ohnehin* ein visuelles Redesign
   bekommen soll; das ist eine Produktentscheidung, keine Framework-Entscheidung, und sollte
   nicht als Nebeneffekt einer Migration passieren.

**Wenn nur eines von beidem gemacht wird: Entscheidung A.** Sie kostet 2–4 PT,
verbessert Betrieb und Ausliefer-Qualität messbar und nimmt keine Option weg.
Entscheidung B kostet das Zwanzigfache und ist reine Innensicht.

---

## Vorgeschlagene Reihenfolge

| # | Schritt | Aufwand | Wert unabhängig vom Rest? |
|---|---|---|---|
| 1 | `Cache-Control` + Kompression für `ServeDir` | 0,5 PT | ja |
| 2 | Tote Deps raus, CSS-Kollisionen fixen, SSE-Kern vereinheitlichen | 2–3 PT | ja |
| 3 | Playwright-Smoke als CI-Job (zunächst nicht-required) | 3–5 PT | ja |
| 4 | `croniq-ui`-Image als zweites Dockerfile-Target + Proxy-Doku + Tag-/Skew-Regel | 2–4 PT | ja |
| 5 | Vue-Port — nur nach expliziter Begründung, Plan neu bepreisen (55–70 PT) | groß | nein |

Schritte 1–4 sind zusammen ~8–12 PT und liefern den Großteil des Nutzens beider Ideen.
