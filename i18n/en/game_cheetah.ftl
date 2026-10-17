tutorial-title = Tutorial
tutorial-target = Tutorial practice game
tutorial-restart = Restart
tutorial-exit = End tutorial
tutorial-error = Tutorial unavailable: { $error }. Restart the tutorial or return to the main menu.
tutorial-safety = The practice game is a small separate process that Game Cheetah starts and closes again when you finish. Only edit memory in your own single-player or offline games.
tutorial-why = What is this about?
tutorial-how = How to do it
tutorial-game-title = Practice game
tutorial-health = Health: { $health } / { $max }
tutorial-damage-button = Take damage
tutorial-heal-button = Drink potion
tutorial-heal-full = Health is already full.
tutorial-game-rules = Damage: −7 to −23 · Potion: +10 to +30 (up to 100) · At 0 health you respawn with 100.
tutorial-progress = Step { $step } of { $total }
tutorial-intro-title = Basics
tutorial-search-title = First search
tutorial-narrow-title = Narrow down
tutorial-edit-title = Change the value
tutorial-freeze-title = Freeze
tutorial-test-title = Test the freeze
tutorial-complete-title = Well done!
tutorial-intro-text =
    Games store numbers such as health, ammo or gold in memory, each at a fixed location: its address. Game Cheetah searches there for the number you currently see in the game.

    The same number usually appears in hundreds of places, though. So you change the value in the game and search again. Only addresses that changed along with it remain.

    Above you can see the practice game; it is already attached. On the left is the regular search area, where you practice in six short steps: search, narrow down, change the value and freeze it.
tutorial-begin = Let's go
tutorial-search-why =
    The first search reads the game's entire memory and remembers every location containing the number. Each of these locations is a candidate.

    To search, you need to know how the number is stored: its data type. In most games health is a whole number, usually of type “int (4 bytes)”.
tutorial-search-how =
    1. Next to the input field, check that “Int” is selected as the type.
    2. Enter the practice game's current health in the “Value:” field.
    3. Click “Search” or press Enter.
tutorial-search-status = Current health: { $health }. So search for { $health }.
tutorial-narrow-why =
    You now have many candidates. Most of them contain the same number only by coincidence. You can recognize the real address because it changes when health changes in the game.

    “Narrow down” no longer scans all memory. It checks only the existing candidates and keeps those that now contain the new value. The list gets shorter with each round.
tutorial-narrow-how =
    1. Change health in the practice game: click “Take damage” or “Drink potion”.
    2. In the same search tab, replace the old value with the new health.
    3. Click “Narrow down”.
    4. Repeat steps 1 to 3 until at most two candidates remain.

    If several results with exactly the same value remain after one round, they are often copies, for example for the display. Only another change separates them from the real health value.
tutorial-narrow-status = Health now: { $health } · Candidates: { $count } (goal: at most { $goal })
tutorial-skip-hint = Some memory locations change along by coincidence. If the list stops getting shorter, you can continue with the remaining candidates.
tutorial-skip = Continue with these candidates
tutorial-edit-why =
    You now know where health is stored. If you write a different value there, the game uses it, because it reads health from exactly this address.

    If two candidates remain, one is often just a copy, for example for the display. The game does not calculate with it; changing it has no effect.
tutorial-edit-how =
    1. In the result table, click the number in a result's “Value” column.
    2. Replace it with 999 and press Enter.
    3. Look at the practice game's health bar. If it shows 999, you found the right address. If not, try the next result.
tutorial-edit-status = Remaining candidates: { $count }
tutorial-freeze-why = Games change their values constantly. After the next hit, the 999 health would be gone again. When a value is frozen, Game Cheetah writes it back several times per second and immediately undoes the game's changes.
tutorial-freeze-how =
    1. In the result table, find the result you used to set health to 999.
    2. Click the snowflake in its “Frozen” column. It becomes highlighted.

    Freezing keeps the value that is in memory at the moment you click.
tutorial-test-why = A highlighted snowflake alone proves nothing. What matters is what happens in the game. So always test whether the frozen value really holds.
tutorial-test-how =
    1. Click “Take damage” in the practice game.
    2. Watch the health bar. It drops briefly and jumps straight back to 999.
tutorial-test-status = Checking whether health returns to 999…
tutorial-complete-text = You completed the whole workflow: you searched for a value, narrowed down the candidates step by step, told the real value apart from a copy, changed it and verified the freeze. This is exactly how you proceed in real games.
tutorial-tips-title = Tips for real games
tutorial-tips =
    • If you find nothing, try another data type. Some games store values as “float (4 bytes)”, a decimal number.
    • If you do not know the exact number, for example a health bar without digits, use the “Unknown” type and narrow down with “increased” or “decreased”.
    • After restarting a game, values usually live at different addresses. Simply search again.
    • Use a separate search tab for each value, for example one for gold and one for ammo.
tutorial-glossary-title = Terms explained
tutorial-glossary =
    Address: The location in memory where a value is stored, shown as a hexadecimal number.
    Value: The number currently stored at that address.
    Data type: How a value is stored. Int is a 4-byte whole number; float is a 4-byte decimal number.
    Candidate: An address that has matched every search so far.
    Narrow down: Check only the existing candidates again with a new value.
    Freeze: Keep writing a value back so the game cannot change it.
tutorial-hint-search-missed = Health is not among the results. Check that the type is “Int” and that you entered exactly the current health. Then click “Reset search” and search again.
tutorial-hint-narrow-lost = Oops, the real address was filtered out. This happens when the value you entered does not match the current health. Click “Undo” and narrow down again with the current value.
tutorial-hint-nothing-removed = Nothing was filtered out because the value has not changed since the last search. First change health in the practice game, then narrow down with the new value.
tutorial-hint-tab-closed = This lesson's search tab was closed. Simply start a new search with the current health.
tutorial-hint-copy-edited = You changed a copy: it now shows 999, but the game's real health did not change. Games often store values more than once. Set the other candidate to 999.
tutorial-hint-wrong-freeze = You froze a different address. Freeze the result you used to set the game's health to 999.
tutorial-hint-freeze-value = Health is no longer 999. First set the right result back to 999, then freeze it. If it is already frozen, release it by clicking its snowflake.
tutorial-hint-freeze-failed = Health did not return to 999. Check that the snowflake of the right result is active and test again.
error-process-title = Process no longer available.
error-process-help = Start the game and select it again.
error-access-title = Access denied.
error-access-help = Check process permissions; see Details for guidance.
error-read-title = Memory cannot be read.
error-read-help = Check the running game process and access permissions.
error-input-title = Invalid value.
error-input-help = Check the input and data type.
error-address-title = Invalid address or write failed.
error-address-help = Find the value again instead of continuing to edit this address.
error-other-title = Action failed.
error-other-help = Check Details for the cause; nothing was automatically retried.
error-select-process = Select process
error-new-search = New search tab
error-edit-value = Correct input
error-access-linux = Linux: run the game and Game Cheetah as the same user. If access is denied, check ptrace and sandbox restrictions. No system settings are changed automatically.
error-access-macos = macOS: signing, debugger permissions or protected processes may restrict access. Check the application's permissions; do not disable system protection indiscriminately.
error-access-windows = Windows: check the permission levels of the game and Game Cheetah. Protected processes may block access. Do not bypass protection mechanisms.
empty-results-undo = Undo last narrowing
persistence-label = Persistence (UNSAFE)
persistence-description = Experimental: shows Save/Load, the address editor and pointer scanner. No restart guarantee: saved chains may point to wrong but still readable memory. Independently verify values before editing or freezing. Disabling cancels these tasks and clears tabs containing saved address definitions, including Undo; files are kept.
persistence-disabled = Persistence is disabled. This experimental feature can be enabled in Settings.
auto-save-progress = Saving: finding pointer chains automatically · Target { $index }/{ $count } · { $mib } MiB
auto-save-cancel = Cancel saving
auto-save-dismiss = Dismiss notice
auto-save-cancelled = Saving cancelled. The previous file is unchanged.
auto-save-error = Not saved: { $error }. The previous file is unchanged.
auto-save-busy = Automatic pointer search for saving is already running.
auto-save-done = Saved: { $count } automatically found pointer chains; { $absolute } absolute addresses (current game session only). Found chains have not been checked after a restart. Load uses the saved chains; current search tabs remain unchanged.
auto-save-limited = Automatic search was limited or memory regions were unreadable. At most 8 distinct numeric targets, 512 MiB each. The manual pointer scanner offers further options.
pointer-scan-title = Pointer scanner
pointer-scan-menu = Find pointer chains…
pointer-scan-help = Select a working numeric result, then search. After restarting the game, find the value again and open this dialog on the NEW result; use “Filter candidates”. Matching values alone do not prove a stable chain.
pointer-scan-target = Target: { $address } ({ $kind })
pointer-scan-new = Search new chains
pointer-scan-filter = Filter candidates
pointer-scan-cancel = Cancel scan
pointer-scan-close = Close scanner
pointer-scan-depth = Maximum dereferences
pointer-scan-offset = Maximum offset (hex)
pointer-scan-budget = Scan budget (MiB)
pointer-scan-limit = Candidate limit
pointer-scan-aligned = Aligned pointers only
pointer-scan-readonly = Include read-only mappings
pointer-scan-negative = Also search negative offsets
pointer-scan-limits = Bounded search: up to 4 million indexed pointers, 50,000 paths and 2 million edges per depth. Default: writable memory and aligned little-endian pointers. Anonymous BSS is not guessed to belong to a module. Not finding a chain does not prove that none exists.
pointer-scan-progress = Read { $mib } MiB · { $pointers } pointers · depth { $depth } · { $count } candidates
pointer-scan-result = { $count } candidates · { $failed } failed memory reads
pointer-scan-truncated = A search limit was reached. These are partial results; increase settings or narrow the target.
pointer-scan-unverified = Candidates are not automatically restart-safe. Allocator/library paths can be accidental. Repeat filtering across restarts and different game situations.
pointer-scan-adopt = Add to new tab
pointer-scan-save = Save candidates
pointer-scan-load = Load candidates
pointer-scan-saved = Candidates saved separately from the cheat table.
pointer-scan-loaded = Candidates loaded. Select the current target and filter before use.
pointer-scan-options-error = Invalid scan settings: depth 1–6, offset 0–10000 hex, budget 1–1024 MiB, candidate limit 1–512.
pointer-scan-numeric-only = Pointer scanning requires a fixed-width numeric result.
pointer-scan-identity-error = Cannot identify the target executable. Check process access permissions.
pointer-scan-wrong-target = The process or value type changed. Select a current result from the same executable before filtering or adopting candidates.
pointer-scan-file-error = Invalid or unsupported pointer candidate file.
pointer-scan-cancelled = Scan cancelled. Previous candidates were kept.
pointer-scan-worker-error = The pointer scan ended without a result.
pointer-scan-no-target = Right-click a current result and choose “Find pointer chains…” to select the target.
pointer-scan-tab = Pointer result
pointer-mode = Pointer chain
pointer-width = Target pointer width (not value type)
pointer-offsets = Pointer offsets in order (hex, separated by commas)
pointer-help = Read a pointer at module + base offset, add the first offset, repeat for each further offset. The final address holds the value. Use 0 for one dereference without an offset. Select the target's 32/64-bit pointer size explicitly; pointers are little-endian.
pointer-depth = A pointer chain needs 1 to 16 offsets.
pointer-needs-process = Pointer resolution requires a connected process.
pointer-read-error = Pointer step { $step } at { $address } could not be read: { $error }
pointer-null = Null pointer at step { $step }; the object may not be loaded yet.
pointer-overflow = Pointer or offset exceeds the selected address range.
pointer-target-error = Pointer target { $address } is not fully readable: { $error }
pointer-trace = Resolved pointer steps
pointer-refresh = Check chain now
pointer-scan-warning = This tab contains pointer definitions. Use a separate search tab for scans and result filters; moving objects must not be filtered at stale addresses.
pointer-safety = A changed or broken pointer target stops its freeze. Check the newly resolved entry before enabling it again.
address-summary = { $relative } module-relative, { $pointers } pointer chains, { $absolute } absolute (not restart-safe), { $pending } unresolved
address-freeze-stopped = Freeze at { $address } stopped: address resolution changed or memory is no longer accessible. Check the entry before enabling it again.
address-save-idle = Finish or cancel running searches before saving.
address-save-help = Saves module addresses and existing chains directly. Automatically searches pointer chains for absolute numeric addresses. Cancellable; without a match, addresses remain valid only for this game session.
address-edit = Edit address…
address-editor-title = Address definition
address-absolute = Absolute
address-relative = Module-relative
address-module = Module (path or unique filename)
address-offset = Offset (hex)
address-value = Address (hex)
address-apply = Apply address
address-cancel = Cancel
address-resolved = Resolved address: { $address }
address-absolute-warning = Absolute addresses are not restart-safe. Use a known pointer chain for heap values.
address-relative-warning = Module offsets survive relocation, not necessarily game updates. Anonymous heap/BSS regions are not assigned to modules automatically.
address-module-missing = Select a module.
address-invalid-hex = Invalid or overflowing hexadecimal address: { $value }
address-map-error = Cannot read process modules: { $error }
address-module-not-found = Module not loaded: { $module }
address-module-ambiguous = Module is ambiguous: { $module }. Use an exact path or a single loaded image.
address-outside-module = Address or value width lies outside the module's readable mappings.
address-duplicate = This address and data type already exist in this tab.
address-unresolved = Unresolved addresses ({ $count })
address-retry = Resolve modules again
address-stale-absolute = Old absolute address. Edit and confirm a valid address for this process.
address-no-process = No process attached.
address-changed = Address resolution changed. Reload or resolve the table before writing.
attach-button = Start
about-button = About
settings-button = Settings
discuss-button = Discuss
bug-button = Bug / feature request
quit-button = Quit

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
search-layout = Layout
search-layout-tabs = All searches as tabs
search-layout-horizontal = Side by side
search-layout-vertical = Stacked
search-layout-grid = Grid
search-layout-hint = Drag a search tab to an edge to split, or onto another tab bar to group. Drag dividers to resize.
search-tab-drag-hint = Drag to another position, an edge or another tab bar to rearrange
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
many-results-hint = Many hits? Change the value in the game and narrow the search above.

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
search-read-failed = Search failed: no target memory could be read. Check the process and access permissions. The previous search state has been restored; released freezes remain off.
search-process-exited = Process '{ $name }' has exited or been replaced. Running searches were cancelled and their previous state restored. Freezes have been cleared.
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
guess-type-hint = Tries Int32 plus Float32 and Float64. Int64 replaces Int32 only for integers outside −2147483648 through 2147483647. Select Int64 explicitly for small values in 64-bit variables. This does not identify the game's variable type.

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
search-scan-statistics = Read { $bytes } in { $elapsed } s ({ $rate }/s); failed/partial reads: { $incomplete }
search-scan-summary = Last scan completed: { $statistics }; matches: { $matches }
search-snapshot-summary = Snapshot captured: { $statistics }
search-scan-statistics-hint = Successful read volume includes chunk overlap, grouped reads and retries. Failed/partial reads count read operations, not unique regions or discarded results; fallback reads may recover candidates. Throughput is averaged over the entire operation, including comparisons and stability observation.

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
confirm-value-writes-label = Write values only on Enter
confirm-value-writes-description = In the result table, Enter writes the edited value; Escape or leaving the field discards the input. Off by default: valid input is written immediately. The memory inspector keeps its existing Enter-to-write behavior.
float-tolerance-label = Float search tolerance
float-tolerance-description = How far Float32 and Float64 matches (including Number / Guess) may differ from the entered value: by less than a fixed number, or by a percentage of the value. The default ±1 lets typed whole numbers find rounded game values (100 matches 99.6, not 101). Use a percentage for large or abbreviated values. Integers always match exactly.
float-tolerance-absolute = Number
float-tolerance-percent = Percent
float-tolerance-chip = Float tolerance
float-tolerance-hint-absolute = Float32 and Float64 hits differ from the searched value by less than this amount.
float-tolerance-hint-percent = Float32 and Float64 hits differ from the searched value by at most this percentage.
float-tolerance-hint-target = Searched value
float-tolerance-hint-settings = Applies to the last search or update. Click to change it for the next one.
float-tolerance-popup-hint = Applies from the next search or update. Also available in Settings.
result-deviation-tooltip = Difference between the current value and the searched value. It changes when the game changes the value.
result-confirm-edit-label = Confirm with Enter
result-confirm-edit-tooltip = Edit value (F2). Enter writes; Escape or leaving the field discards the input. Failed writes keep the input for correction.
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

# Compact Save/Load notices
notice-details = Details
notice-file = File: { $path }
notice-saved = Saved { $count } entries · { $absolute } absolute (session only)
notice-loaded = Loaded { $count } entries · { $unresolved } unresolved
notice-unverified = Pointers unverified after restart
notice-save-failed = Save failed. Check the process and file/memory access.
notice-load-failed = Load failed. Check that the file exists, is a valid table, and matches the process; check access.
notice-chain-risk = A readable pointer chain can still point to the wrong value. Verify targets after each restart before editing or freezing.
