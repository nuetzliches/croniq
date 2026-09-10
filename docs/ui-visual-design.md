# Visuelle Gestaltung — Bestandsaufnahme und Richtung

Stand: 2026-09-10.

Dieses Dokument existiert, weil es gefehlt hat.
[ADR-0004](adr/0004-vue-rebuild-for-the-dashboard.md) hielt fest, die
Design-Phase liefere „das Design-System, die Komponenten-Bibliothek und das
Screen-Inventar". Geliefert wurden Bibliothek (Nuxt UI 4) und Inventar
([`ui-screen-inventory.md`](ui-screen-inventory.md)). **Das Design selbst hat
nie jemand entschieden** — und weil es auf keiner Liste stand, sah die Lücke
nach „kommt noch" aus statt nach „dafür ist niemand zuständig".

Der Vue-Baum ist **kein Prototyp**. ADR-0004 macht ihn zum Ersatz des
ausgelieferten Dashboards; er muss dessen Niveau erreichen.

---

## Bestandsaufnahme

Ich hatte das React-Dashboard bis jetzt nur im Quelltext gelesen. Das ist keine
Grundlage für „kein Rückschritt", also habe ich es aufgenommen und angesehen —
alle elf Screens, angemeldet, mit den Demo-Daten des Dev-Stacks.
`ui/scripts/capture-screens.mjs` macht das reproduzierbar, für beide Bäume.

### Was es gut macht — die Liste, hinter die nicht zurückgefallen werden darf

1. **Cards-Chrome auf Farbverlauf-Grund.** Sidebar, Topbar und Inhalt sind
   abgerundete Karten mit Außenabstand auf einem violett-nach-blau laufenden
   Grund. Das ist eigenständig und sieht nicht nach Bootstrap-Admin aus.
2. **Eine eigene KPI-Sprache.** Versale Mikro-Beschriftung mit Sperrung, große
   Zahl, erklärende Unterzeile, eingebettete Sparkline. Konsequent auf
   Dashboard und Job-Detail.
3. **Tag-Chips mit Zähler** (`env=demo 5`, `kind=ops 2`) als Filterleiste.
4. **Job-Zeilen tragen ihre Historie.** Jede Zeile in der Job-Liste zeigt eine
   Balken-Sparkline der letzten Läufe plus Erfolgsquote. Sehr viel Information
   auf sehr wenig Fläche.
5. **Definitionslisten als Detail-Schiene** — Beschriftung links, Wert rechts,
   monospace wo es Werte sind. Hervorragend zu überfliegen.
6. **Die Konsole.** Dunkles Terminal-Panel im hellen Chrome, Zeitstempel /
   Level / Target / Nachricht in Spalten, strukturierte Felder gedimmt
   angehängt, Level-Chips mit Farbpunkt. Der stärkste Screen.
7. **Login als Produktseite.** Schlagzeile („Schedule. Observe. Recover."),
   Positionierungstext, **echte Live-Kennzahlen aus dem öffentlichen
   `/health`** und ein animiertes Terminal. Man sieht vor dem Anmelden, dass
   der Server lebt.
8. **Command-Palette** (Ctrl K) in der Topbar.
9. Status-Pillen, monospace IDs als Links, relative Zeiten, rechtsbündige
   Dauern.

### Wo es schwach ist

1. **Master/Detail wird uniform angewandt, auch wo es nichts zu zeigen gibt.**
   Auf *Executions* steht das Detail-Panel leer und beansprucht ~60 % der
   Fläche; auf *Runners* mit einem Runner sind es ~85 %. Der Farbverlauf
   dominiert dann eine Fläche, die er nicht rahmt, sondern füllt.
2. **Die Executions-Liste ist die unwirtschaftlichste Fläche der App.**
   Jede Ausführung ist eine dreizeilige Karte (~88 px) — sichtbar sind sieben.
   Als Tabelle wären es fünfundzwanzig. Und weil es keine Spalten gibt, kann
   man Dauern nicht untereinander vergleichen. Ausgerechnet dieser Screen wird
   laut Inventar die *eine* Ausführungsansicht.
3. **Natives `<select>`** als Statusfilter — unstilisiert, bricht mit allem
   anderen auf der Seite.
4. **Spalten brechen um.** „29m ago" läuft im Job-Detail auf zwei Zeilen, in
   jeder Zeile. Die Dichte ist gewollt, aber nicht zu Ende gerechnet.
5. **Die Detail-Schiene wird unten abgeschnitten**, ohne dass etwas darauf
   hinweist, dass unterhalb noch Inhalt liegt.

Zusammengefasst: **starke Informationsgestaltung, schwache Layout-Robustheit.**
Das ist eine gute Ausgangslage — das Schwierige ist da, das Fehlende ist
handwerklich.

---

## Übernommen, nicht entschieden

Die Produktidentität existiert und wird nicht neu erfunden, solange niemand das
Gegenteil beschließt:

- **Marke:** das Orbit-Zeichen aus `public/icons/mark-mono.svg`, portiert als
  `app/components/BrandMark.vue`.
- **Markenfarbe:** `#6A54DF` als Ramp mit 500 auf dem Markenwert.
- **Icon-Satz:** vollständig übernommen, inklusive Manifest.

---

## Richtung

### Behalten

Cards-Chrome samt Grund, die KPI-Sprache, die Tag-Chips, die Historie in
Listenzeilen, die Definitionslisten, die Konsole als dunkles Terminal, und den
Login als Produktseite mit echten Kennzahlen.

### Ändern

- **Listen sind Listen.** Master/Detail nur, wo das Detail auch gebraucht wird.
  Runs wird eine echte Tabelle mit Spalten; das Detail öffnet als Seitenpanel
  oder eigene Route, statt dauerhaft zwei Drittel der Fläche leer zu belegen.
- **Eine Dichte, festgelegt an einer Stelle.** Die Tabelle im Job-Detail hat
  die richtige Dichte; sie wird der Maßstab, inklusive Spaltenbreiten, die
  nicht umbrechen.
- **Leerzustände verdienen ihre Fläche.** Ein Symbol plus zwei Zeilen in einem
  60-%-Panel ist kein Leerzustand, sondern eine Lücke mit Beschriftung.
- **Filter sind Komponenten**, keine nativen Steuerelemente.

### Neue Impulse

Der Punkt, den ich beim Ansehen am stärksten vermisst habe:

**Croniq zeigt überall die Vergangenheit und nirgends die Zukunft.** Es ist ein
Scheduler — das Interessanteste ist, was *gleich* passiert. „NEXT FIRE in 33s"
existiert genau einmal, im Detail eines einzelnen Jobs. Eine kompakte
Vorschau-Schiene („was feuert in der nächsten Stunde") wäre echter neuer Wert
und nutzt Daten, die der Server über `/v1/jobs/states` und den Forecast bereits
liefert.

Weitere Kandidaten, schwächer begründet:

- **Die Failure-Heatmap zur Hauptantwort machen.** Sie beantwortet „ist etwas
  kaputt" besser als jede Zahl, ist aber klein, unbeschriftet und steht unten
  rechts.
- **Tastatur zuerst.** Ctrl K gibt es; `j`/`k` in Listen und Enter zum Öffnen
  passen zu einem Betriebswerkzeug und kosten wenig.
- **Dichte-Umschalter** (kompakt/komfortabel). Betreiber sind sich hier
  uneinig, und die Entscheidung muss nicht global fallen.
- **Dunkelmodus gleichwertig.** Heute ist die App hell mit einer dunklen
  Konsole. Für ein Werkzeug, in dem Leute Stunden verbringen, ist das eine
  offene Frage, keine Antwort.

---

## Vorgehen

In Durchgängen, mit Haltepunkten — nicht in einem Rutsch. Genau das
Nacheinander gibt Gelegenheit, gegenzusteuern, bevor eine Entscheidung in
sieben Screens steckt.

| # | Inhalt | Ergebnis, an dem man es beurteilen kann |
|---|---|---|
| 1 | Bestandsaufnahme und Richtung | dieses Dokument |
| 2 | Fundament: Chrome, Grund, Dichte, Typografie, Zustände (leer/lädt/Fehler) | Shell und Login sehen aus wie das Produkt |
| 3 | Runs als erster echter Screen | die Richtung ist an der schwierigsten Liste bewiesen |
| 4 | Dashboard, Runners, Dead Letters | die Bausteine tragen |
| 5 | Jobs, danach der Rest | — |

Nach jedem Durchgang: Aufnahmen beider Bäume nebeneinander
(`node ui/scripts/capture-screens.mjs react|vue …`), damit „kein Rückschritt"
eine Feststellung bleibt und keine Behauptung wird.

---

## Nachtrag: was das Testnetz nicht sieht

Die Playwright-Suite prüft Routen, Session, URL-Verträge und SSE — nichts davon
Optik. Das ist Absicht ([ADR-0004](adr/0004-vue-rebuild-for-the-dashboard.md),
Scope-Guard 2), hatte aber eine Konsequenz: das Theme war eine Zeit lang **gar
nicht aktiv**. `@theme` statt `@theme static` ließ Tailwind die gesamte
Farb-Ramp wegoptimieren, sodass jedes `bg-primary` transparent auflöste. Alle
Verhaltensprüfungen blieben grün.

`ui-vue/app/lib/theme.test.ts` schließt die billige Hälfte dieser Lücke. Die
teure Hälfte — ob es *gut aussieht* — schließt kein Test, sondern Hinsehen.
Deshalb das Aufnahme-Skript.
