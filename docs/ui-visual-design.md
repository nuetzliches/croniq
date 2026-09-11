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
| 2 ✓ | Fundament: Chrome, Grund, Dichte, Typografie, Zustände (leer/lädt/Fehler) | Shell und Login sehen aus wie das Produkt |
| 3 ✓ | Runs als erster echter Screen | die Richtung ist an der schwierigsten Liste bewiesen |
| 4 ✓ | Dashboard, Runners, Dead Letters | die Bausteine tragen |
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

---

## Durchgang 2 — was entschieden wurde

**Grund.** Getönt, nicht weiß, mit zwei schwachen Radialverläufen in
gegenüberliegenden Ecken. Der erste Versuch war 9 % Marke auf Weiß und damit
unsichtbar; die Ursache lag aber tiefer als die Deckkraft — bei weißem Grund
*und* weißen Karten gibt es nichts, worüber die Karten schweben könnten. Jetzt
ist der Grund getönt und die Karten sind opak.

**Dunkel ist gleichwertig.** Nicht die invertierte Helligkeit, sondern ein
eigener Grund aus denselben zwei Ankern mit anderen Gewichten. Das React-Baum
ist hell mit eingebetteter dunkler Konsole; für ein Werkzeug, das den ganzen
Tag offen ist, war das die Frage wert.

**Dichte an einer Stelle.** `--cq-row-h` (2,375 rem), `--cq-cell-x`, `--cq-cell-y`
und die Utilities `cq-label` / `cq-num`. Die Maße stammen aus der Tabelle im
Job-Detail des React-Baums, die als einzige die richtige Dichte hat. `cq-num`
setzt `tabular-nums` *und* `white-space: nowrap` — letzteres gegen genau den
Defekt, den das Audit fand: „29m ago" auf zwei Zeilen in jeder Zeile.

**Zustände.** `AppEmpty`, `AppLoading`, `AppError`. Zwei Entscheidungen darin,
die über Kosmetik hinausgehen: `AppEmpty` hat einen `action`-Slot, weil ein
Leerzustand, der nur Leere meldet, den einen Moment verschenkt, in dem jemand
sicher nach einem nächsten Schritt sucht. Und `AppError` bietet „Try again"
**nicht** bei 403 und 404 an — eine Schaltfläche, die eine abschließende
Antwort zu wiederholen verspricht, erzieht dazu, sie sinnlos zu drücken.
Serverseitige Fehlermeldungen bleiben erhalten; Croniqs API sagt Nützliches,
und das durch ein freundliches Allgemeines zu ersetzen macht das eigene Backend
schwerer zu betreiben.

**Login als Produktseite.** Schlagzeile, Positionierung und drei Kacheln mit
echten Zahlen aus dem öffentlichen `/health`, plus Build-Zeile. Übernommen,
weil die erste Frage bei selbst gehosteter Software „läuft das überhaupt" ist
und diese Seite sie beantworten kann, bevor man Zugangsdaten hat.

**Nicht übernommen:** das simulierte Terminal, das einen Befehl tippt, den
niemand ausführt. Es ist charmant und unecht, und die ehrliche Fassung
derselben Idee — der tatsächliche Serverzustand — steht bereits daneben.

### Offen geblieben

- ~~Dichte-Umschalter~~ und ~~Tastatur~~ — beides in Durchgang 3 gelandet.
- **Command-Palette.** Der React-Baum hat sie, dieser noch nicht.
- **Die Vorschau-Schiene** („was feuert als Nächstes") aus den neuen Impulsen —
  gehört zum Dashboard, also Durchgang 4.

---

## Durchgang 3 — Runs

Der Screen, der drei ersetzt, und im React-Baum der schwächste war. Gemessen
statt behauptet:

| | React | Vue |
|---|---|---|
| Zeilenhöhe | ~88 px (dreizeilige Karten) | **38 px**, alle 200 identisch |
| Sichtbare Läufe | 7 | ~20 **mit geöffnetem Detail** |
| Spalten | keine | State, Job, Run, Runner, Fired, Duration |
| Detail-Panel leer | belegt ~60 % | wird gar nicht erst gerendert |
| Statusfilter | natives `<select>` | `USelectMenu` |

**Das Detail ersetzt die Liste nicht, es steht daneben.** `/executions/:id`
rendert dieselbe Komponente — der Link bleibt teilbar, die Liste behält Scroll
und Filter. Verifiziert: nach dem Zeilenklick steht die URL auf
`/executions/<id>?state=completed`, das Panel ist da, die Tabelle auch.

**Die Reaktivitätsfalle.** `useExecutions` nimmt einen *Getter*, keinen Wert.
Das ist das im Migrationsplan meistgenannte Risiko: mit einem einfachen Objekt
frieren Query-Key und Anfrage auf dem ersten Render ein, die Seite rendert neu
und zeigt stillschweigend die alten Zeilen. `toValue` in `queryKey` *und*
`queryFn` ist, was die Abfrage erneut laufen lässt.

**Ein neuer Impuls eingelöst:** Tastaturnavigation (`j`/`k`, Enter, Escape —
und sie stiehlt keine Tasten aus Eingabefeldern). Sie brauchte eine Liste, um
sinnvoll zu sein.

> **Nachtrag (Durchgang 5).** Hier stand ein zweiter Impuls: ein
> Dichte-Umschalter unter `croniq_density`. Der ist wieder draußen — die
> Begründung steht unten.


**Ein Befund beim Ansehen:** die Run-Spalte brach mit dem Attempt-Marker `#2`
auf zwei Zeilen um — genau die Fehlerklasse, gegen die `cq-num` gebaut wurde,
nur hatte ich das Utility auf dieser Zelle vergessen. Jetzt sind alle 200
Zeilen exakt 38 px.

### Noch offen an diesem Screen

- **Zeitfenster-Filter.** Der Server kann `since`/`until`, die Oberfläche nicht.
- **Nachladen.** Es wird hart auf 200 Zeilen begrenzt; ältere Läufe sind nicht
  erreichbar, und ein Deep-Link auf einen älteren Lauf findet ihn nicht — das
  Panel sagt das ehrlich, statt leer zu bleiben.
- **Verlinkung** von Job und Runner in die jeweiligen Screens, sobald es sie
  gibt.

---

## Durchgang 4 — Dashboard, Runners, Dead Letters

**Die Vorschau-Schiene ist da, und sie kostete keine Server-Arbeit.**
`/v1/dashboard/forecast` existiert seit jeher — das React-Dashboard ruft ihn nur
nie auf, allein die Jobs-Seite tut es. Der schärfste Audit-Befund („zeigt
überall die Vergangenheit, nirgends die Zukunft") war also eine fehlende
Abfrage, keine fehlenden Daten.

Die Schiene nutzt zwei Quellen, absichtlich: `jobs/states` liefert den exakten
nächsten Feuerzeitpunkt je Job — das, was ein Betreiber liest —, der Forecast
die Form der nächsten Stunde in Buckets, also „und dann wird es voll". Eine
Liste allein verbirgt die Last, ein Histogramm allein die Namen. Überfällige
Jobs stehen darüber und nicht mittendrin: sie sind nicht „demnächst", sie sind
zu spät.

**Dashboard** wie im Inventar beschlossen — Statusboard plus *ausschließlich*
Fehlschläge, keine allgemeine Lauf-Liste. Die wäre das vierte Rendering
derselben Tabelle gewesen. Die Failure-Heatmap ist aus der unteren rechten Ecke
nach oben gewandert und hat Wochentags- und Stundenbeschriftung bekommen; sie
beantwortet „ist etwas kaputt" besser als jede Zahl daneben.

**Runners** verliert das Master/Detail. Mit einem Runner waren im React-Baum
~85 % der Fläche ein leeres Panel. Alles, was das Detail zeigte, ist entweder
ein Feld, das in die Zeile passt, oder eine Lauf-Liste — und Läufe leben jetzt
an einem Ort, also verlinkt die Zeile dorthin. Der SSE-Kern aus #585 bekommt
hier seinen ersten Verbraucher, mit `shallowRef` für die Zeilen: jeder Frame
ersetzt das ganze Array.

**Dead Letters** behält den eigenen Screen, wie entschieden. Neu gegenüber der
React-Fassung: eine abgelehnte Wiedervorlage zeigt die Begründung des Servers.
Der Stale-Replay-Guard lehnt ab, wenn der logische Feuerzeitpunkt älter ist als
die Policy erlaubt — das ist eine Entscheidung, kein Fehler, und „Replay
failed" verschweigt sie.

### Beim Ansehen gefunden

Das Durchsatz-Diagramm war leer, obwohl „186 runs" danebenstand: Prozenthöhen
in einem Zwischen-`div` ohne definierte Höhe lösen zu null auf. Sichtbar nur
durch Hinsehen — kein Test hätte das gemeldet.

## Durchgang 5 — Dichte und Scrollverhalten

Zwei Korrekturen an bereits Gebautem, beide aus dem Betrachten heraus.

### Der Dichte-Umschalter ist wieder draußen

Er war in Durchgang 3 als „neuer Impuls" eingezogen, und das war die falsche
Einordnung. Ein Dichte-Regler gibt dem Lesenden ein Problem zurück, das das
Design hätte lösen sollen — und er verdoppelt die Arbeit dauerhaft: jede
künftige Tabelle muss in zwei Dichten richtig aussehen, sonst ist eine davon
die schlechtere. `comfortable` gewinnt, weil diese Zeilen eine Status-Pille und
monospaced Ids tragen; beides braucht den Durchschuss.

Die Zeilenhöhe steht jetzt als `@utility cq-row` in `main.css` statt als
Konstante in drei Views. Das war vorher dreimal derselbe Kommentar — ein
verlässliches Zeichen, dass die Entscheidung eine Ebene zu tief lag.

Der Schlüssel `croniq_density` wird nicht mehr gelesen und nicht mehr
geschrieben; ein Rest im `localStorage` eines Entwicklerbrowsers ist folgenlos.

### Es scrollt die Liste, nicht die Seite

Filter, Spaltenköpfe und der Wartungsbanner wanderten beim Scrollen mit nach
oben. Die Ursache ist dieselbe Klasse wie das leere Durchsatz-Diagramm aus
Durchgang 4, nur andersherum: die Shell war `min-h-screen`. Eine
*Mindest*höhe ist keine definite Höhe, also löste `h-full` in jeder Listenseite
zu `auto` auf, ihr `overflow-auto`-Container wuchs mit dem Inhalt statt zu
scrollen — und was dann scrollte, war das Dokument.

Die Kette, damit es trägt:

| Ebene | vorher | jetzt |
| --- | --- | --- |
| Shell-Wurzel | `min-h-screen` | `h-screen overflow-hidden` |
| `<main>` | `overflow-y-auto`, Block | `flex flex-col min-h-0 overflow-hidden` |
| Wartungsbanner | scrollt mit | `shrink-0`, steht |
| Scrollbereich | — | ein `min-h-0 flex-1 overflow-y-auto` um `<RouterView>` |

Dieser eine Bereich bedient beide Seitenformen, und das ist der Grund, ihn in
der Shell zu haben statt in jeder Seite: eine dokumentförmige Seite (das
Dashboard) ist höher als die Box und scrollt darin; eine listenförmige Seite
setzt `h-full`, ist damit exakt die Box, nichts läuft über, und das einzige,
was sich bewegt, ist der Tabellenkörper in seinem eigenen Rahmen. Sidebar,
Kopfzeile und die Filterleiste über der Liste stehen in beiden Fällen.

## Durchgang 6 — Jobs

Der dickste Screen, und der, an dem die Bestandsaufnahme am deutlichsten war:
sechs Tabs, zwei davon dieselbe Executions-Tabelle mit unterschiedlichem
Zeilenlimit.

### Sechs Tabs auf zwei

| Tab | wohin |
| --- | --- |
| Overview | bleibt — und nimmt *Schedule* auf |
| Executions | entfällt; Kopfzeile verlinkt `/executions?job_key=…` |
| Schedule | in die Übersicht; es sind vier Felder |
| DSL | bleibt |
| Alerts | `/alerts` |
| Audit | Audit-Log in den Einstellungen |

*Schedule* in die Übersicht zu holen war die Entscheidung mit dem größten
Effekt: die Regel eines Jobs ist das Erste, was jemand wissen will, der einen
Job öffnet. Sie einen Klick entfernt zu halten machte die Übersicht zu einer
Feldliste, in der genau das Wichtige fehlte.

### Die Liste beantwortet jetzt die Frage, für die man sie öffnete

Die React-Liste war eine Spalte Namen. „Wann läuft das nächste Mal" und „ist
etwas zu spät" beantwortete man, indem man Jobs einzeln aufmachte. Drei
Endpunkte hatten die Antwort und wurden hier nie zusammengeführt:
`/v1/jobs` (Definition), `/v1/jobs/states` (nächster/letzter Lauf, `overdue`,
Lebenszyklus) und `/v1/schedules` (die Regeln). Die neue Liste joint sie und
sortiert überfällige nach oben.

Neu gegenüber der React-Fassung ist außerdem, dass *Quelle* eine Spalte ist:
Croniqfile-verwaltet oder API-verwaltet. Vorher war das eine Eigenschaft, die
man erst bemerkte, wenn ein Button ausgegraut war.

### Drei Befunde beim Hinsehen

**Die Tag-Chips tragen nicht.** Aus der Runner-Liste übernommen, wo eine Flotte
eine Handvoll Tags hat. Jobs sind nach Team, Umgebung *und* Art getaggt — schon
die Demo hat sieben, und die Reihe schob Zähler und Primäraktion aus der
Werkzeugleiste. Jetzt ein Menü, dessen Breite nicht von der Tag-Anzahl abhängt.

**„just now" ist in einer Zukunftsspalte die falsche Zeitform.**
`formatRelative` sagt das innerhalb seiner Fünf-Sekunden-Schwelle, und in der
Spalte *Next fire* liest es sich als Vergangenheit. Dort steht jetzt „due now".

**Spalten, die neben dem offenen Detail nicht passen, werden weggelassen statt
abgeschnitten.** Mit geöffnetem Detail war die Tabelle rechts hart beschnitten
— der Container scrollt zwar horizontal, aber ohne sichtbaren Hinweis. *Last
fire*, *Fires* und *Source* verschwinden jetzt, solange ein Job offen ist.

Und eine Korrektur an der eigenen Dichte-Entscheidung aus Durchgang 5: die
Job-Zeile ist die einzige zweizeilige im Produkt (Key über Beschreibung) und
bekommt mit 49 px die Höhe, die zwei Zeilen brauchen. `cq-row` ist für
Tabellenzeilen ohnehin ein Minimum, kein Fixwert — genau dafür.

### Geprüft

`ui/scripts/vue-write-paths.mjs` fährt zwölf Schreibpfade gegen den Dev-Stack:
anlegen, Schedule anhängen, deaktivieren/aktivieren, triggern, pausieren/
fortsetzen, bearbeiten, DSL rendern, adoptieren, löschen. Alle zwölf grün, und
die Ablehnung der Adoption kommt im Wortlaut des Servers an:

> DSL adoption is disabled — set `policy { dsl_adopt_on_mutate true }` in the
> Croniqfile to enable

Dazu zehn Unit-Tests für `renderDsl`.

### Noch offen an diesem Screen

- **Der Vue-Baum hat keine e2e-Suite.** Das Abnahmekriterium aus ADR-0004 ist
  die Playwright-Suite, die aber gegen das *gebaute* React-Dashboard auf 4233
  läuft. Bis der Vue-Baum dort ein eigenes Projekt hat, ist
  `vue-write-paths.mjs` ein Notbehelf und heißt in seinem Kopf auch so.
- **Zwei `401` beim Kaltstart.** Nicht aus dieser Arbeit, aber hier gemessen:
  `runRefresh` wiederholt einmal, weil ein anderer Tab den Refresh-Token
  rotiert haben könnte. Beim allerersten Besuch gibt es gar kein Cookie, und
  der Wiederholungsversuch ist garantiert vergeblich. Server und Client
  unterscheiden „kein Cookie präsentiert" und „Cookie abgelehnt" beide nicht —
  der Kommentar im Handler nennt das sogar ausdrücklich als Absicht. Eigener
  Vorgang.
- **Kalender-Auswahl im Schedule-Editor** listet Namen, aber es gibt noch
  keinen Kalender-Screen, auf dem man einen anlegen könnte (Durchgang 7).

