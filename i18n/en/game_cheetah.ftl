attach-button = Start
about-button = About
settings-button = Settings
discuss-button = Discuss
bug-button = Bug / feature request
quit-button = Quit
main-menu-subtitle = Memory scanner and game trainer

process-label = Process:
filter-processes-hint = Search by name, PID or command …
process-selection-title = Select a process
process-selection-subtitle = Choose a running instance to scan and edit its memory.
process-connect-button = Connect
process-cancel-button = Cancel
process-select-hint = No process selected
process-group-select-hint = Expand the group and select an instance
process-selection-gone = The selected process has exited. Please select another process.
process-keyboard-hint = ⏶ ⏷ Select · ⏴ ⏵ Group · Enter Connect · Ctrl/Cmd+F Search · Esc Cancel
process-refresh-status = Automatic · 1 s
process-group-count = { $groups } groups
process-group-count-hint = Instances of the same executable are grouped together. The process count includes individual instances, not groups.
process-group-badge = { $count } processes
process-group-filtered-badge = { $matched }/{ $total } processes
process-group-memory-hint = Sum of resident memory (RSS) of all instances, even with an active filter. Shared pages may be counted more than once.
process-expand-hint = Expand or collapse group
process-sort-hint = Click to sort; click again to reverse the direction
process-copy-pid = Copy PID
process-copy-command = Copy command
no-processes-available-hint = No matching user processes are available. The list refreshes automatically.
no-processes-loading = Loading processes…
no-processes-loading-hint = Reading the process list — this may take a moment.
no-processes-match = No processes match the current filter.
no-processes-match-hint = Try a different filter, or clear it to see every process.
reset-filter-button = Clear filter
process-count-total = { $total } processes
process-count-filtered = { $shown } of { $total } processes
first-search-label = First search
searches-heading = Searches
rename-search-hint = F2 or right-click to rename
rename-search-menu = Rename
close-search-menu = Close
close-other-searches-menu = Close others

search-label = Search { $search }
name-label = Name:
value-label = Value:
search-description-label = Search description
search-value-label = Search for { $valuetype } value

found-results-label =
    { $results ->
        [1] found one result.
       *[other] found { $results } results.
    }
result-unit-singular = result
result-unit-plural = results

no-results-label = No results found.
empty-search-title = No search yet
empty-search-hint = Enter a value to start searching.
empty-results-title = No results
empty-results-hint = Change the value or data type and try again.
too-many-results-hint = Too many hits to look through. Change the value in the game and narrow the search using the controls above.

undo-button = Undo
numeric-filter-title = Filter results
result-filter-combined-hint = Keep results matching all enabled filters.
result-filter-numeric = Value
result-filter-types = Data types
result-filter-types-hint = Keep the selected type interpretations. Does not change a result's type or write memory.
result-filter-types-all = All
result-filter-types-none = None
result-filter-no-types = Select at least one numeric data type.
result-filter-no-criteria = Enable at least one filter.
result-filter-stable = Stable values only
result-filter-seconds = seconds from Apply
result-filter-stable-hint = Observe every matching result after Apply. Discard any value whose bytes change or become unreadable. Samples are taken about every 200 ms (slower for large lists); changes between samples can be missed. This is not a retrospective check. Other searches' freezes can make values appear stable.
result-filter-duration-error = Choose an observation duration from 1 to 30 seconds.
result-filter-stability-progress = Observing stable values: { $current } / { $total } s
result-filter-cancel = Cancel and restore results
numeric-filter-description = Keep matching current values from all results. Does not write memory; releases this search's freezes. Undo restores results, not freezes.
numeric-filter-value = Value
numeric-filter-upper = Upper bound
numeric-filter-between = between
numeric-filter-and = and
numeric-filter-apply = Apply filter
numeric-filter-invalid = Enter an Int64 integer or a finite decimal number (decimal point, optional exponent).
numeric-filter-reversed = The lower bound must not exceed the upper bound.
numeric-filter-hint = Between includes both bounds. Comparisons are exact (Float32 bounds are rounded to Float32). Unreadable, non-numeric and non-finite values are excluded. Unknown-search comparisons keep their previous baseline.
initial-search-button = Search
update-button = Narrow down
clear-button = Reset search
capture-snapshot-button = Capture initial state
snapshot-ready-label = Initial state captured
reset-search-tooltip = Reset results and search history and release values frozen by this search.
result-live-edit-label = Live editing
result-live-edit-tooltip = Valid input is written to the process immediately. Enter ends editing. Escape also ends editing but does not undo values already written.
result-keyboard-hint = ⏶ ⏷ Select · F2 Edit · Del Remove
result-edit-tooltip = Edit value directly (F2). Valid input is written immediately.
result-unreadable = Unreadable
result-unreadable-tooltip = This memory could not be read. It may have been freed or the process may have exited.
result-frozen-count = frozen
close-button = Close
hide-results-button = Hide results
show-results-button = Show results
rename-button = Rename
edit-button = Edit
remove-button = Remove
copy-address-menu = Copy address
copy-value-menu = Copy value
open-memory-editor-menu = Open in memory editor

add-search-button = Add

save-cheat-table-button = Save
load-cheat-table-button = Load

generic-error-label = <error>
invalid-input-error = Invalid input
invalid-number-error = Invalid number
conversion-error = Error converting { $valuetype }: { $message }

integer-range-error = Outside the range of { $kind } ({ $min } to { $max }). Check the data type in the memory editor; a wider type can overwrite adjacent values.
integer-input-error = Invalid integer: { $value }
check-result-type = Check data type…
type-width-warning = A type change reinterprets memory; it does not enlarge the game's variable. Writing a wider type can overwrite adjacent values.
guess-type-hint = Tries Int32, Int64, Float32 and Float64. Several types may match the same address; this does not identify the game's variable type.

guess-value-item = number (4-8 bytes)
byte-value-item = byte (1 byte)
short-value-item = short (2 bytes)
int-value-item = int (4 bytes)
int64-value-item = int64 (8 bytes)
float-value-item = float (4 bytes)
double-value-item = double (8 bytes)
string-value-item = string

guess-descr = Number
byte-descr = byte
short-descr = short
int-descr = int
int64-descr = int64
float-descr = float
double-descr = double
string-descr = string

address-heading = Address
value-heading = Value
freezed-heading = Frozen
freeze-all-tooltip = Click to freeze or unfreeze every result
freeze-result-tooltip = Freeze result
unfreeze-result-tooltip = Unfreeze result
datatype-heading = Data type

pid-heading = PID
name-heading = Name
memory-heading = Memory
user-heading = User
command-heading = Command

update-numbers-progress = Update { $current }/{ $total }…
search-memory-progress = Search { $current }/{ $total }…

tab-hover-text=Double click for rename
close-tab-hover-text=Close active search
open-tab-hover-text=Open new search

about-dialog-title=About Game Cheetah
about-dialog-heading = Game Cheetah
about-dialog-description = 
    Game cheetah is an utility to modifiy the state of a game process.

    Make yourself more memory, better stats or more lifes.

    Single player games store the game state in memory where multi player games
    don't. So, this utility is not useful for multiplayer games.
    
    Game Cheetah runs natively on Linux, Mac and Windows computers.
    
    Keep in mind that altering a game memory contents may lead to game and/or computer crashes. Use at your own risk.
about-dialog-ok=OK
about-dialog-created_by = Created by { $authors }

unknown-value-item = Unknown
unknown-descr = Unknown value
compare-label = Compare:
decreased-button = Decreased
increased-button = Increased
changed-button = Changed
unchanged-button = Unchanged
search-type-label = Search type:
unknown-search-description = Compare memory values without knowing the exact value

process-exited-title = Process has exited
process-exited-message = The target process is no longer running. Please return to the main menu to select a new process.
automatic-reconnect-waiting-message = Automatic reconnect is enabled in Settings. Waiting for { $name } to appear again…
back-to-main-button = Back to main menu

# Settings
settings-title = Settings
automatic-reconnect-label = Automatic reconnect
automatic-reconnect-description = When the target process exits, keep watching for a process with the same name and attach again automatically. Disabled by default.
check-for-updates-label = Check for updates
check-for-updates-description = On startup, contact the GitHub releases API once to check whether a newer version is available. No data about you is sent.
update-available = Update available: v{ $version } — click to open release page
config-directory-label = Configuration directory
config-directory-description = Cheat tables are saved here by default. The location is the platform's standard configuration directory (provided by the dirs crate) with a game-cheetah subfolder.
open-config-directory-button = Open
copy-config-directory-button = Copy path

# Memory editor
memory-editor-nav-back = Previous address
memory-editor-nav-forward = Next address
memory-editor-origin-button = Original address
memory-editor-origin-tooltip = Return to the address where the editor was opened
memory-editor-redo-button = Redo
memory-editor-follow-pointer = Open address
memory-editor-follow-pointer-tooltip = Jump to the readable pointer target. Use Back to return to the previous address.
memory-editor-pointer-unavailable = No readable pointer target in the known memory map
memory-editor-inspector-title = Inspector
memory-editor-inspector-select-hint = Right-click a byte in the grid to inspect it
memory-editor-inspector-edit-hint = Enter writes, Esc cancels
memory-editor-inspector-type-label = Type:
memory-editor-inspector-type-tooltip = Reinterpret the current result as a different numeric type
memory-editor-inspector-variable-type-tooltip = Variable-length results cannot be reinterpreted from the editor
memory-editor-inspector-endian-tooltip = Switch byte order
memory-editor-inspector-unsigned = Unsigned
memory-editor-inspector-signed = Signed
memory-editor-inspector-float-raw = Float / raw
memory-editor-inspector-readable-hint = Needs { $count } readable bytes
memory-editor-inspector-write-failed = Writing to 0x{ $address } failed. Check the process and write permissions. Your input is kept for another attempt.
memory-editor-title = Memory editor
memory-editor-pid = PID { $pid }
memory-editor-address-label = Address
memory-editor-address-hint = 0x…
memory-editor-go-button = Go
memory-editor-ascii-heading = ASCII
memory-editor-no-regions = No readable memory regions
memory-editor-region-label = Region
memory-editor-region-unmapped = no mapped region
memory-editor-region-unnamed = <unnamed>
memory-editor-region-anonymous = anonymous
memory-editor-undo-tooltip = Undo last memory write (Ctrl+Z)
memory-editor-redo-tooltip = Redo memory write (Ctrl+Y / Ctrl+Shift+Z)
memory-editor-from-hit-label = from search hit
memory-editor-access-unmapped = unmapped
memory-editor-access-rwx = read / write / execute
memory-editor-access-rw = read / write
memory-editor-access-rx = read / execute
memory-editor-access-r = read-only
memory-editor-access-w = write-only
memory-editor-access-x = execute-only
memory-editor-access-none = no access

# Memory editor errors
memory-editor-error-cursor-not-readable = Cursor is not in a readable memory region
memory-editor-error-read-map = Failed to read memory map of PID { $pid }: { $error }
memory-editor-error-no-regions = PID { $pid } reports no readable memory regions
memory-editor-error-attach = Failed to attach to process: { $error }
memory-editor-error-read-address = Failed to read 0x{ $address }: { $error }
memory-editor-error-write-address = Failed to write 0x{ $address }: { $error }
memory-editor-error-invalid-value = Invalid { $kind } value '{ $input }': { $error }
memory-editor-error-out-of-range-max = { $kind } value { $value } is out of range (max { $max })
memory-editor-error-out-of-range = { $kind } value { $value } is out of range ({ $min }..={ $max })
