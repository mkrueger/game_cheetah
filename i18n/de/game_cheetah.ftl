error-process-title = Prozess nicht mehr verfügbar.
error-process-help = Spiel starten und erneut auswählen.
error-access-title = Zugriff verweigert.
error-access-help = Prozessberechtigungen prüfen; Hinweise unter Details.
error-read-title = Speicher nicht lesbar.
error-read-help = Laufenden Spielprozess und Zugriffsrechte prüfen.
error-input-title = Ungültiger Wert.
error-input-help = Eingabe und Datentyp prüfen.
error-address-title = Adresse ungültig oder Schreiben fehlgeschlagen.
error-address-help = Wert neu suchen, statt diese Adresse weiter zu bearbeiten.
error-other-title = Aktion fehlgeschlagen.
error-other-help = Ursache unter Details prüfen; nichts wurde automatisch erneut versucht.
error-select-process = Prozess auswählen
error-new-search = Neuer Such-Tab
error-edit-value = Eingabe korrigieren
error-access-linux = Linux: Spiel und Game Cheetah mit demselben Benutzer ausführen. Bei verweigertem Zugriff ptrace- und Sandbox-Beschränkungen prüfen. Es werden keine Systemeinstellungen automatisch verändert.
error-access-macos = macOS: Prozesszugriff kann durch Signatur, Debugger-Berechtigungen oder geschützte Prozesse beschränkt sein. Berechtigungen der Anwendung prüfen; Systemschutz nicht pauschal abschalten.
error-access-windows = Windows: Berechtigungsstufen von Spiel und Game Cheetah prüfen. Geschützte Prozesse können den Zugriff blockieren. Keine Schutzmechanismen umgehen.
empty-results-undo = Letzte Eingrenzung zurücknehmen
persistence-label = Persistenz (UNSAFE)
persistence-description = Experimentell: Blendet Save/Load, Adresseditor und Pointer-Scanner ein. Keine Neustartgarantie: Gespeicherte Ketten können auf falschen, weiterhin lesbaren Speicher zeigen. Werte vor Änderungen oder Freeze unabhängig prüfen. Deaktivieren bricht diese Aufgaben ab und leert Tabs mit gespeicherten Adressdefinitionen samt Undo; Dateien bleiben erhalten.
persistence-disabled = Persistenz ist deaktiviert. Diese experimentelle Funktion kann in den Einstellungen aktiviert werden.
auto-save-progress = Speichern: Pointer-Ketten automatisch suchen · Adresse { $index }/{ $count } · { $mib } MiB
auto-save-cancel = Speichern abbrechen
auto-save-dismiss = Hinweis schließen
auto-save-cancelled = Speichern abgebrochen. Die bisherige Datei bleibt unverändert.
auto-save-error = Nicht gespeichert: { $error }. Die bisherige Datei bleibt unverändert.
auto-save-busy = Die automatische Suche beim Speichern läuft bereits.
auto-save-done = Gespeichert: { $count } automatisch gefundene Pointer-Ketten; { $absolute } absolute Adressen (nur aktueller Spielstart). Gefundene Ketten sind noch nicht nach einem Neustart geprüft. Laden verwendet die gespeicherten Ketten; die aktuellen Suchtabs bleiben unverändert.
auto-save-limited = Die automatische Suche war begrenzt oder Speicherbereiche waren nicht lesbar. Maximal 8 verschiedene numerische Ziele, je 512 MiB. Weitere Optionen bietet der manuelle Pointer-Scanner.
pointer-scan-title = Pointer-Scanner
pointer-scan-menu = Pointer-Ketten suchen…
pointer-scan-help = Einen funktionierenden numerischen Treffer wählen und suchen. Nach dem Spielneustart den Wert erneut finden und diesen Dialog am NEUEN Treffer öffnen; dann „Kandidaten filtern“. Gleiche Werte allein beweisen keine stabile Kette.
pointer-scan-target = Ziel: { $address } ({ $kind })
pointer-scan-new = Neue Ketten suchen
pointer-scan-filter = Kandidaten filtern
pointer-scan-cancel = Suche abbrechen
pointer-scan-close = Scanner schließen
pointer-scan-depth = Maximale Dereferenzierungen
pointer-scan-offset = Maximaler Offset (hex)
pointer-scan-budget = Scanbudget (MiB)
pointer-scan-limit = Kandidatenlimit
pointer-scan-aligned = Nur ausgerichtete Pointer
pointer-scan-readonly = Schreibgeschützte Bereiche einbeziehen
pointer-scan-negative = Auch negative Offsets suchen
pointer-scan-limits = Begrenzte Suche: bis zu 4 Millionen indizierte Pointer, 50.000 Pfade und 2 Millionen Kanten je Tiefe. Standard: schreibbarer Speicher und ausgerichtete Little-Endian-Pointer. Anonyme BSS-Bereiche werden keinem Modul auf Verdacht zugeordnet. Keine Treffer bedeutet nicht, dass keine Kette existiert.
pointer-scan-progress = { $mib } MiB gelesen · { $pointers } Pointer · Tiefe { $depth } · { $count } Kandidaten
pointer-scan-result = { $count } Kandidaten · { $failed } fehlgeschlagene Speicherlesevorgänge
pointer-scan-truncated = Ein Suchlimit wurde erreicht. Dies sind Teilergebnisse; Einstellungen erhöhen oder das Ziel weiter eingrenzen.
pointer-scan-unverified = Kandidaten sind nicht automatisch neustartfest. Pfade über Speicherverwaltung/Systembibliotheken können zufällig passen. Den Abgleich über Neustarts und verschiedene Spielsituationen wiederholen.
pointer-scan-adopt = In neuen Tab übernehmen
pointer-scan-save = Kandidaten speichern
pointer-scan-load = Kandidaten laden
pointer-scan-saved = Kandidaten separat von der Cheat-Tabelle gespeichert.
pointer-scan-loaded = Kandidaten geladen. Aktuelles Ziel wählen und vor der Verwendung filtern.
pointer-scan-options-error = Ungültige Scanparameter: Tiefe 1–6, Offset 0–10000 hex, Budget 1–1024 MiB, Kandidatenlimit 1–512.
pointer-scan-numeric-only = Die Pointer-Suche benötigt einen numerischen Treffer mit fester Breite.
pointer-scan-identity-error = Die Programmdatei des Zielprozesses konnte nicht bestimmt werden. Zugriffsrechte prüfen.
pointer-scan-wrong-target = Prozess oder Werttyp geändert. Vor Abgleich oder Übernahme einen aktuellen Treffer desselben Programms wählen.
pointer-scan-file-error = Ungültige oder nicht unterstützte Pointer-Kandidatendatei.
pointer-scan-cancelled = Suche abgebrochen. Bisherige Kandidaten wurden beibehalten.
pointer-scan-worker-error = Die Pointer-Suche wurde ohne Ergebnis beendet.
pointer-scan-no-target = Einen aktuellen Treffer rechtsklicken und „Pointer-Ketten suchen…“ wählen, um das Ziel festzulegen.
pointer-scan-tab = Pointer-Treffer
pointer-mode = Pointer-Kette
pointer-width = Pointerbreite des Zielprozesses (nicht Werttyp)
pointer-offsets = Pointer-Offsets in Reihenfolge (hex, durch Kommas getrennt)
pointer-help = Pointer bei Modul + Basisoffset lesen, ersten Offset addieren, für jeden weiteren Offset wiederholen. Die letzte Adresse enthält den Wert. 0 bedeutet einmal dereferenzieren ohne Offset. Die 32-/64-Bit-Pointerbreite des Zielprozesses explizit wählen; Pointer sind Little-Endian.
pointer-depth = Eine Pointer-Kette benötigt 1 bis 16 Offsets.
pointer-needs-process = Pointer-Auflösung benötigt einen verbundenen Prozess.
pointer-read-error = Pointer-Schritt { $step } bei { $address } konnte nicht gelesen werden: { $error }
pointer-null = Null-Pointer bei Schritt { $step }; das Objekt ist möglicherweise noch nicht geladen.
pointer-overflow = Pointer oder Offset überschreitet den gewählten Adressbereich.
pointer-target-error = Pointer-Ziel { $address } ist nicht vollständig lesbar: { $error }
pointer-trace = Aufgelöste Pointer-Schritte
pointer-refresh = Kette jetzt prüfen
pointer-scan-warning = Dieser Tab enthält Pointer-Definitionen. Für Suchen und Trefferfilter einen separaten Suchtab verwenden; bewegliche Objekte dürfen nicht an veralteten Adressen gefiltert werden.
pointer-safety = Ein geändertes oder ungültiges Pointer-Ziel beendet den Freeze. Den neu aufgelösten Eintrag vor erneutem Aktivieren prüfen.
address-summary = { $relative } modulrelativ, { $pointers } Pointer-Ketten, { $absolute } absolut (nicht neustartfest), { $pending } nicht aufgelöst
address-freeze-stopped = Freeze bei { $address } beendet: Adressauflösung geändert oder Speicher nicht mehr zugänglich. Eintrag vor erneutem Aktivieren prüfen.
address-save-idle = Vor dem Speichern bitte laufende Suchen abschließen oder abbrechen.
address-save-help = Speichert Moduladressen und vorhandene Ketten direkt. Für absolute numerische Adressen werden automatisch Pointer-Ketten gesucht. Abbrechbar; ohne Treffer bleiben Adressen nur für diesen Spielstart gültig.
address-edit = Adresse bearbeiten…
address-editor-title = Adressdefinition
address-absolute = Absolut
address-relative = Modulrelativ
address-module = Modul (Pfad oder eindeutiger Dateiname)
address-offset = Offset (hex)
address-value = Adresse (hex)
address-apply = Adresse übernehmen
address-cancel = Abbrechen
address-resolved = Aufgelöste Adresse: { $address }
address-absolute-warning = Absolute Adressen sind nicht neustartfest. Für Heap-Werte eine bekannte Pointer-Kette verwenden.
address-relative-warning = Moduloffsets überstehen Verschiebungen, aber nicht unbedingt Spielupdates. Anonyme Heap-/BSS-Bereiche werden nicht automatisch Modulen zugeordnet.
address-module-missing = Bitte ein Modul auswählen.
address-invalid-hex = Ungültige oder zu große hexadezimale Adresse: { $value }
address-map-error = Prozessmodule können nicht gelesen werden: { $error }
address-module-not-found = Modul nicht geladen: { $module }
address-module-ambiguous = Modul ist mehrdeutig: { $module }. Einen genauen Pfad oder ein einziges geladenes Abbild verwenden.
address-outside-module = Adresse oder Wertbreite liegt außerhalb der lesbaren Modulbereiche.
address-duplicate = Diese Adresse mit diesem Datentyp existiert bereits in diesem Tab.
address-unresolved = Nicht aufgelöste Adressen ({ $count })
address-retry = Module erneut auflösen
address-stale-absolute = Alte absolute Adresse. Eine gültige Adresse für diesen Prozess bearbeiten und bestätigen.
address-no-process = Kein Prozess verbunden.
address-changed = Adressauflösung geändert. Tabelle vor dem Schreiben neu laden oder erneut auflösen.
attach-button = Start
about-button = Über
settings-button = Einstellungen
discuss-button = Diskutieren
bug-button = Fehler melden
quit-button = Beenden
main-menu-subtitle = Speicherscanner und Game Trainer

process-label = Prozess:
filter-processes-hint = Nach Name, PID oder Kommando suchen …
process-selection-title = Prozess auswählen
process-selection-subtitle = Wähle eine laufende Instanz zum Durchsuchen und Bearbeiten ihres Speichers.
process-connect-button = Verbinden
process-cancel-button = Abbrechen
process-select-hint = Noch kein Prozess ausgewählt
process-group-select-hint = Gruppe öffnen und eine Instanz auswählen
process-selection-gone = Der ausgewählte Prozess wurde beendet. Bitte erneut auswählen.
process-keyboard-hint = ⏶ ⏷ Auswählen · ⏴ ⏵ Gruppe · Enter Verbinden · Strg/Cmd+F Suche · Esc Abbrechen
process-refresh-status = Automatisch · 1 s
process-group-count = { $groups } Gruppen
process-group-count-hint = Instanzen derselben ausführbaren Datei werden gruppiert. Der Prozesszähler zählt einzelne Instanzen, nicht Gruppen.
process-group-badge = { $count } Prozesse
process-group-filtered-badge = { $matched }/{ $total } Prozesse
process-group-memory-hint = Summe des residenten Speichers (RSS) aller Instanzen, auch bei aktivem Filter. Gemeinsam genutzte Seiten können mehrfach gezählt sein.
process-expand-hint = Gruppe auf- oder zuklappen
process-sort-hint = Zum Sortieren klicken; erneut klicken, um die Richtung umzukehren
process-copy-pid = PID kopieren
process-copy-command = Kommando kopieren
no-processes-available-hint = Keine passenden Benutzerprozesse verfügbar. Die Liste wird automatisch aktualisiert.
no-processes-loading = Prozesse werden geladen…
no-processes-loading-hint = Die Prozessliste wird gelesen — das kann einen Moment dauern.
no-processes-match = Keine Prozesse passen zum aktuellen Filter.
no-processes-match-hint = Probiere einen anderen Filter oder leere ihn, um alle Prozesse zu sehen.
reset-filter-button = Filter zurücksetzen
process-count-total = { $total } Prozesse
process-count-filtered = { $shown } von { $total } Prozessen
first-search-label = Erste Suche 
searches-heading = Laufende Suchen
rename-search-hint = F2 oder Rechtsklick zum Umbenennen
rename-search-menu = Umbenennen
close-search-menu = Schließen
close-other-searches-menu = Andere schließen

search-label = Suche { $search }
name-label = Name:
value-label = Wert:
search-description-label = Suchbeschreibung
search-value-label = Suche nach { $valuetype }

found-results-label =
    { $results ->
        [1] 1 Vorkommen gefunden
       *[other] { $results } Vorkommen gefunden
    }
result-unit-singular = Treffer
result-unit-plural = Treffer

no-results-label = Keine Vorkommen
empty-search-title = Noch keine Suche
empty-search-hint = Wert eingeben und die erste Suche starten.
empty-results-title = Keine Treffer
empty-results-hint = Wert ändern oder Datentyp wechseln und erneut suchen.
too-many-results-hint = Zu viele Treffer zum Durchsehen. Ändere den Wert im Spiel und grenze die Suche oben weiter ein.

undo-button = Rückgängig
numeric-filter-title = Treffer eingrenzen
result-filter-combined-hint = Treffer behalten, die alle aktivierten Filter erfüllen.
result-filter-numeric = Wert
result-filter-types = Datentypen
result-filter-types-hint = Nur die gewählten Typinterpretationen behalten. Ändert weder den Datentyp eines Treffers noch den Speicher.
result-filter-types-all = Alle
result-filter-types-none = Keine
result-filter-no-types = Mindestens einen numerischen Datentyp auswählen.
result-filter-no-criteria = Mindestens einen Filter aktivieren.
result-filter-stable = Nur stabile Werte
result-filter-seconds = Sekunden ab Anwenden
result-filter-stable-hint = Beobachtet alle passenden Treffer ab Anwenden. Werte mit geänderten oder unlesbaren Bytes entfallen. Stichproben etwa alle 200 ms (bei großen Listen langsamer); Änderungen dazwischen können unbemerkt bleiben. Keine rückwirkende Prüfung. Fixierungen anderer Suchen können Werte stabil erscheinen lassen.
result-filter-duration-error = Eine Beobachtungsdauer von 1 bis 30 Sekunden wählen.
result-filter-stability-progress = Beobachte stabile Werte: { $current } / { $total } s
result-filter-cancel = Abbrechen und Treffer wiederherstellen
search-read-failed = Suche fehlgeschlagen: Kein Zielspeicher konnte gelesen werden. Prozess und Zugriffsrechte prüfen. Der vorherige Suchstand wurde wiederhergestellt; gelöste Fixierungen bleiben aus.
search-process-exited = Prozess „{ $name }“ wurde beendet oder ersetzt. Laufende Suchen wurden abgebrochen und ihr vorheriger Zustand wiederhergestellt. Fixierungen wurden aufgehoben.
numeric-filter-description = Passende aktuelle Werte aus allen Treffern behalten. Schreibt keinen Speicher; löst Fixierungen dieser Suche. Rückgängig stellt Treffer wieder her, nicht Fixierungen.
numeric-filter-value = Wert
numeric-filter-upper = Obergrenze
numeric-filter-between = zwischen
numeric-filter-and = und
numeric-filter-apply = Filter anwenden
numeric-filter-invalid = Eine Int64-Ganzzahl oder endliche Dezimalzahl eingeben (Dezimalpunkt, optional Exponent).
numeric-filter-reversed = Die Untergrenze darf nicht größer als die Obergrenze sein.
numeric-filter-hint = Zwischen schließt beide Grenzen ein. Vergleiche sind exakt (Float32-Grenzen werden auf Float32 gerundet). Unlesbare, nichtnumerische und nichtendliche Werte entfallen. Zustandsvergleiche behalten ihren vorherigen Bezugsstand.
initial-search-button = Suchen
update-button = Eingrenzen
clear-button = Suche zurücksetzen
capture-snapshot-button = Ausgangszustand erfassen
snapshot-ready-label = Ausgangszustand erfasst
reset-search-tooltip = Treffer und Suchverlauf zurücksetzen und die von dieser Suche eingefrorenen Werte freigeben.
result-live-edit-label = Live-Bearbeitung
result-live-edit-tooltip = Gültige Eingaben werden sofort in den Prozess geschrieben. Enter beendet die Bearbeitung. Escape beendet sie ebenfalls, macht bereits geschriebene Werte aber nicht rückgängig.
result-keyboard-hint = ⏶ ⏷ Auswahl · F2 Bearbeiten · Entf Entfernen
result-edit-tooltip = Wert direkt bearbeiten (F2). Gültige Eingaben werden sofort geschrieben.
result-unreadable = Nicht lesbar
result-unreadable-tooltip = Dieser Speicherbereich konnte nicht gelesen werden. Möglicherweise wurde er freigegeben oder der Prozess beendet.
result-frozen-count = eingefroren
close-button = Schließen

hide-results-button = Ergebnisse verstecken
show-results-button = Ergebnisse zeigen
rename-button = Umbenennen
edit-button = Bearbeiten
remove-button = Entfernen
copy-address-menu = Adresse kopieren
copy-value-menu = Wert kopieren
open-memory-editor-menu = Im Speichereditor öffnen

add-search-button = Neu

save-cheat-table-button = Speichern
load-cheat-table-button = Laden

generic-error-label = <Fehler>
invalid-input-error = Eingabe ungültig
invalid-number-error = Zahl ungültig
conversion-error = Fehler beim Konvertieren { $valuetype }: { $message }

integer-range-error = Außerhalb des Bereichs von { $kind } ({ $min } bis { $max }). Datentyp im Speichereditor prüfen; ein breiterer Typ kann benachbarte Werte überschreiben.
integer-input-error = Ungültige Ganzzahl: { $value }
check-result-type = Datentyp prüfen…
type-width-warning = Ein Typwechsel interpretiert Speicher neu; er vergrößert nicht die Spielvariable. Das Schreiben eines breiteren Typs kann benachbarte Werte überschreiben.
guess-type-hint = Probiert Int32 sowie Float32 und Float64. Int64 ersetzt Int32 nur bei Ganzzahlen außerhalb von −2147483648 bis 2147483647. Für kleine Werte in 64-Bit-Variablen Int64 explizit wählen. Dies erkennt nicht den Variablentyp des Spiels.

guess-value-item = Zahl (4-8 Bytes)
byte-value-item = Byte (1 byte)
short-value-item = Short (2 Bytes)
int-value-item = Int (4 Bytes)
int64-value-item = Int64 (8 Bytes)
float-value-item = Float (4 Bytes)
double-value-item = Double (8 Bytes)
string-value-item = String

guess-descr = Zahl
byte-descr = Byte
short-descr = Short
int-descr = Int
int64-descr = Int64
float-descr = Float
double-descr = Double
string-descr = String

address-heading = Adresse
value-heading = Wert
freezed-heading = Eingefroren
freeze-all-tooltip = Klicken, um alle Ergebnisse einzufrieren oder freizugeben
freeze-result-tooltip = Ergebnis einfrieren
unfreeze-result-tooltip = Ergebnis freigeben
datatype-heading = Datentyp

pid-heading = PID
name-heading = Name
memory-heading = Speicher
user-heading = Nutzer
command-heading = Kommando

update-numbers-progress = Aktualisiere { $current }/{ $total }…
search-memory-progress = Suche { $current }/{ $total }…

tab-hover-text=Doppelklick zum Umbenennen
close-tab-hover-text=Schließe aktive Suche
open-tab-hover-text=Neue Suche


about-dialog-title=Über Game Cheetah
about-dialog-heading = Game Cheetah
about-dialog-description = 
    Game Cheetah ist ein Tool zur Statusänderung von Computerspielen.

    Ändere den Geldbetrag, bessere Attribute oder Extraleben.

    Einzelspieler-Spiele speichern ihren Status im Hauptspeicher.
    Multplayer-Spiele tun das nicht. Daher ist dieses Tool nur nützlich für
    Einzelspieler.
    
    Game Cheetah unterstützt Linux, Mac und Windows.
    
    Speicheränderungen können zu Spiel oder Computerabstürzen führen. Verwendung auf eigenes Risiko.
about-dialog-ok=OK
about-dialog-created_by = Programmiert von { $authors }

unknown-value-item = Unbekannt
unknown-descr = Unbekannter Wert
compare-label = Vergleichen:
decreased-button = Verringert
increased-button = Erhöht
changed-button = Geändert
unchanged-button = Unverändert
search-type-label = Suchtyp:
unknown-search-description = Speicherwerte vergleichen ohne den genauen Wert zu kennen

process-exited-title = Prozess beendet
process-exited-message = Der Zielprozess läuft nicht mehr. Bitte kehren Sie zum Hauptmenü zurück, um einen neuen Prozess auszuwählen.
automatic-reconnect-waiting-message = Automatische Wiederverbindung ist in den Einstellungen aktiviert. Warte darauf, dass { $name } wieder erscheint…
back-to-main-button = Zurück zum Hauptmenü

# Einstellungen
settings-title = Einstellungen
confirm-value-writes-label = Werte erst mit Enter schreiben
confirm-value-writes-description = In der Trefferliste schreibt Enter den bearbeiteten Wert; Escape oder Verlassen des Feldes verwirft die Eingabe. Standardmäßig aus: Gültige Eingaben werden sofort geschrieben. Der Speicherinspektor behält sein bisheriges Schreiben mit Enter bei.
result-confirm-edit-label = Mit Enter bestätigen
result-confirm-edit-tooltip = Wert bearbeiten (F2). Enter schreibt; Escape oder Verlassen des Feldes verwirft die Eingabe. Fehlgeschlagene Schreibversuche behalten die Eingabe zur Korrektur.
automatic-reconnect-label = Automatische Wiederverbindung
automatic-reconnect-description = Wenn der Zielprozess beendet wird, weiter nach einem Prozess mit demselben Namen suchen und automatisch erneut verbinden. Standardmäßig deaktiviert.
check-for-updates-label = Auf Updates prüfen
check-for-updates-description = Beim Start einmalig die GitHub-Releases-API kontaktieren, um zu prüfen, ob eine neuere Version verfügbar ist. Es werden keine persönlichen Daten gesendet.
update-available = Update verfügbar: v{ $version } — klicken, um die Release-Seite zu öffnen
config-directory-label = Konfigurationsverzeichnis
config-directory-description = Cheat-Tabellen werden standardmäßig hier gespeichert. Der Pfad ist das plattformübliche Konfigurationsverzeichnis (bereitgestellt vom Crate dirs) mit einem Unterordner game-cheetah.
open-config-directory-button = Öffnen
copy-config-directory-button = Pfad kopieren

# Speichereditor
memory-editor-nav-back = Zur vorherigen Adresse
memory-editor-nav-forward = Zur nächsten Adresse
memory-editor-origin-button = Ausgangsadresse
memory-editor-origin-tooltip = Zur Adresse zurückkehren, an der der Editor geöffnet wurde
memory-editor-redo-button = Wiederholen
memory-editor-follow-pointer = Adresse öffnen
memory-editor-follow-pointer-tooltip = Zum lesbaren Zeigerziel springen. Mit Zurück gelangst du wieder zur vorherigen Adresse.
memory-editor-pointer-unavailable = Kein lesbares Zeigerziel im bekannten Speicherbereich
memory-editor-inspector-title = Inspektor
memory-editor-inspector-select-hint = Rechtsklick auf ein Byte zum Untersuchen
memory-editor-inspector-edit-hint = Enter schreibt, Esc bricht ab
memory-editor-inspector-type-label = Typ:
memory-editor-inspector-type-tooltip = Den aktuellen Treffer als anderen numerischen Datentyp interpretieren
memory-editor-inspector-variable-type-tooltip = Treffer mit variabler Länge können im Editor nicht umgedeutet werden
memory-editor-inspector-endian-tooltip = Byte-Reihenfolge wechseln
memory-editor-inspector-unsigned = Ohne Vorzeichen
memory-editor-inspector-signed = Mit Vorzeichen
memory-editor-inspector-float-raw = Gleitkomma / Rohdaten
memory-editor-inspector-readable-hint = Benötigt { $count } lesbare Bytes
memory-editor-inspector-write-failed = Schreiben nach 0x{ $address } fehlgeschlagen. Prüfe Prozess und Schreibrechte. Die Eingabe bleibt für einen erneuten Versuch erhalten.
memory-editor-title = Speichereditor
memory-editor-pid = PID { $pid }
memory-editor-address-label = Adresse
memory-editor-address-hint = 0x…
memory-editor-go-button = Los
memory-editor-ascii-heading = ASCII
memory-editor-no-regions = Keine lesbaren Speicherbereiche
memory-editor-region-label = Bereich
memory-editor-region-unmapped = kein zugeordneter Bereich
memory-editor-region-unnamed = <ohne Namen>
memory-editor-region-anonymous = anonym
memory-editor-undo-tooltip = Letzte Schreiboperation rückgängig machen (Strg+Z)
memory-editor-redo-tooltip = Schreiboperation wiederherstellen (Strg+Y / Strg+Umschalt+Z)
memory-editor-from-hit-label = vom Suchtreffer
memory-editor-access-unmapped = nicht zugeordnet
memory-editor-access-rwx = lesen / schreiben / ausführen
memory-editor-access-rw = lesen / schreiben
memory-editor-access-rx = lesen / ausführen
memory-editor-access-r = nur lesen
memory-editor-access-w = nur schreiben
memory-editor-access-x = nur ausführen
memory-editor-access-none = kein Zugriff

# Speichereditor – Fehler
memory-editor-error-cursor-not-readable = Cursor befindet sich nicht in einem lesbaren Speicherbereich
memory-editor-error-read-map = Speicherkarte für PID { $pid } konnte nicht gelesen werden: { $error }
memory-editor-error-no-regions = PID { $pid } meldet keine lesbaren Speicherbereiche
memory-editor-error-attach = Verbinden mit Prozess fehlgeschlagen: { $error }
memory-editor-error-read-address = Lesen von 0x{ $address } fehlgeschlagen: { $error }
memory-editor-error-write-address = Schreiben nach 0x{ $address } fehlgeschlagen: { $error }
memory-editor-error-invalid-value = Ungültiger { $kind }-Wert '{ $input }': { $error }
memory-editor-error-out-of-range-max = { $kind }-Wert { $value } liegt außerhalb des Bereichs (max { $max })
memory-editor-error-out-of-range = { $kind }-Wert { $value } liegt außerhalb des Bereichs ({ $min }..={ $max })

# Kompakte Speichern-/Laden-Meldungen
notice-details = Details
notice-file = Datei: { $path }
notice-saved = { $count } Einträge gespeichert · { $absolute } absolut (nur diese Sitzung)
notice-loaded = { $count } Einträge geladen · { $unresolved } nicht aufgelöst
notice-unverified = Pointer nach Neustart ungeprüft
notice-save-failed = Speichern fehlgeschlagen. Prozess sowie Datei-/Speicherzugriff prüfen.
notice-load-failed = Laden fehlgeschlagen. Datei vorhanden, gültige Tabelle und passender Prozess? Zugriffsrechte prüfen.
notice-chain-risk = Eine lesbare Pointer-Kette kann trotzdem auf den falschen Wert zeigen. Ziele nach jedem Neustart vor dem Ändern oder Einfrieren überprüfen.
