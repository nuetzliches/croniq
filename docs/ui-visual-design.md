# Visuelle Gestaltung — offene Punkte

Stand: 2026-09-10.

Dieses Dokument existiert, weil es gefehlt hat.
[ADR-0004](adr/0004-vue-rebuild-for-the-dashboard.md) hielt fest, die
Design-Phase liefere „das Design-System, die Komponenten-Bibliothek und das
Screen-Inventar". Geliefert wurden Bibliothek (Nuxt UI 4) und Inventar
([`ui-screen-inventory.md`](ui-screen-inventory.md)). **Das Design selbst hat
nie jemand entschieden** — und weil es auf keiner Liste stand, war die Lücke
nicht sichtbar, sondern sah aus wie „kommt noch".

Der Vue-Baum ist **kein Prototyp**. ADR-0004 sagt, er ersetzt das ausgelieferte
Dashboard; er muss also Auslieferqualität erreichen. Aktuell ist er ein
funktionierendes Gerüst im Standardaussehen von Nuxt UI.

## Was übernommen wurde — keine Entscheidung, sondern Vorgabe

Die Produktidentität existiert und wird nicht neu erfunden, solange niemand das
Gegenteil beschließt:

- **Marke:** das Orbit-Zeichen aus `public/icons/mark-mono.svg`, als
  `app/components/BrandMark.vue` portiert (mit `useId` für die Mask-ID, weil
  die React-Fassung eine konstante DOM-ID nutzt und zwei Zeichen auf einer
  Seite kollidieren).
- **Markenfarbe:** `#6A54DF`, als Ramp `--color-brand-50…950` mit 500 auf dem
  Markenwert. Nuxt UIs `primary` zeigt darauf. Vorher stand dort `blue` — ein
  Platzhalter, den ich gesetzt und nicht als Entscheidung gekennzeichnet hatte.
- **Icon-Satz:** vollständig aus `ui/public/icons/` übernommen, inklusive
  Manifest und Apple-Touch-Icon.

## Was offen ist

Keine dieser Fragen ist beantwortet, und keine beantwortet sich beim Bauen von
selbst.

### 1. Login-Seite

Der React-Login ist eine inszenierte Seite: Bühne, simulierte Konsole,
rotierende Schlagzeile, Statistik-Kacheln (`login-hero`, `login-stage`,
`login-console`, `login-stats` in `ui/src/styles/login.css`). Der Vue-Login hat
den vollständigen Flow und **keine** Gestaltung — eine zentrierte Karte.

Das war eine Auslassung, keine Entscheidung. Zu klären: trägt die Login-Seite
weiterhin das Gesicht des Produkts, oder ist sie ein Formular? Beides ist
vertretbar; ein selbst gehostetes Werkzeug wird von Betreibern benutzt, nicht
von Besuchern beworben.

### 2. Dichte und Typografie

Das React-Dashboard ist dicht: 11,5–12,5 px in Tabellen, schmale Zeilenhöhen,
viel auf einem Bildschirm. Nuxt UIs Vorgaben sind großzügiger. Für eine
Betriebsoberfläche, in der Läufe und Runner in Listen gelesen werden, ist das
eine spürbare Entscheidung — und sie fällt einmal, in der Theme-Konfiguration,
oder hundertmal verstreut in den Screens.

### 3. Theme-Umschalter

Funktioniert und persistiert unter `croniq_theme` (System/Hell/Dunkel), ist
aber ein nacktes `USelect` in der Topbar. Zu klären: bleibt es eine
Auswahlliste, oder wird es ein Umschalter wie im Referenzprojekt (Sonne/Mond
mit View-Transition)? Der Referenz-Umschalter ist hübsch und kostet eine
Animation samt `::view-transition`-Regeln.

### 4. Dunkelmodus als erste Klasse

Croniq ist ein Werkzeug für Betreiber; viele arbeiten dauerhaft dunkel. Der
React-Baum behandelt Dunkel als gleichwertig. Ob Nuxt UIs Dunkelvarianten dafür
ohne Nacharbeit reichen, ist ungeprüft.

### 5. Leerzustände, Ladezustände, Fehlerzustände

Das React-Dashboard hat eine eigene `EmptyState`-Komponente und einen
Marken-Spinner (`BrandMark spinning`). Im Vue-Baum existiert bisher keiner von
beiden. Diese drei Zustände machen den Großteil des Eindrucks aus, den eine
frische Installation hinterlässt — dort ist alles leer.

## Wie das eingeplant wird

Zwei Wege, und das ist eine echte Wahl:

**A — ein Gestaltungsdurchgang vor den Screens.** Login, Dichte, Zustände und
Theme einmal festlegen, danach bauen die Screens dagegen. Teurer im Vorlauf,
aber die Screens entstehen nur einmal.

**B — pro Screen mitentscheiden.** Schneller sichtbar, führt aber
erfahrungsgemäß dazu, dass Screen 1 den Ton setzt und Screen 7 nachgezogen
werden muss.

Ungeachtet dessen: die Punkte oben gehören als Issues geführt, sonst
wiederholt sich genau der Fehler, aus dem dieses Dokument entstanden ist.

## Nachtrag: was das Testnetz nicht sieht

Die Playwright-Suite prüft Routen, Session, URL-Verträge und SSE — nichts
davon Optik. Das ist Absicht ([ADR-0004](adr/0004-vue-rebuild-for-the-dashboard.md),
Scope-Guard 2), hat aber eine Konsequenz, die beim Bauen sichtbar wurde: das
Theme war eine Zeit lang **gar nicht aktiv** — `@theme` statt `@theme static`
ließ Tailwind die gesamte Farb-Ramp wegoptimieren, sodass jedes `bg-primary`
transparent auflöste. Alle Verhaltensprüfungen blieben grün.

Wer Optik nicht prüft, merkt nicht, wenn sie fehlt. Ein Screenshot-Vergleich
ist für ein Ein-Personen-Projekt überdimensioniert; eine einzelne Zusicherung,
dass die Markenfarbe ankommt, ist es nicht.
