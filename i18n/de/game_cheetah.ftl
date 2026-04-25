attach-button = Start
about-button = Über
discuss-button = Diskutieren
bug-button = Fehler melden
quit-button = Beenden

process-label = Prozess:
filter-processes-hint = Prozesse filtern
first-search-label = Erste Suche 
searches-heading = Laufende Suchen

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

no-results-label = Keine Vorkommen

undo-button = Rückgängig
initial-search-button = Erste Suche
update-button = Aktualisieren
clear-button = Löschen
close-button = Schließen
hide-results-button = Ergebnisse verstecken
show-results-button = Ergebnisse zeigen
rename-button = Umbenennen
edit-button = Bearbeiten
remove-button = Entfernen

add-search-button = Neu

generic-error-label = <Fehler>
invalid-input-error = Eingabe ungültig
invalid-number-error = Zahl ungültig
conversion-error = Fehler beim Konvertieren { $valuetype }: { $message }

guess-value-item = Zahl (2-8 Bytes)
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

address-heading = Addresse
value-heading = Wert
freezed-heading = Eingefroren
datatype-heading = Datentyp

pid-heading = Pid
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
back-to-main-button = Zurück zum Hauptmenü

# Speichereditor
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
