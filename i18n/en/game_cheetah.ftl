attach-button = Start
about-button = About
discuss-button = Discuss
bug-button = Bug/Feature request
quit-button = Quit

process-label = Process:
filter-processes-hint = Filter processes
first-search-label = First search
searches-heading = Searches

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

no-results-label = No results found.

undo-button = Undo
initial-search-button = Initial search
update-button = Update
clear-button = Clear
close-button = Close
hide-results-button = Hide Results
show-results-button = Show Results
rename-button = Rename
edit-button = Edit
remove-button = Remove

add-search-button = Add

generic-error-label = <error>
invalid-input-error = Invalid input
invalid-number-error = Invalid number
conversion-error = Error converting { $valuetype }: { $message }

guess-value-item = number (2-8 bytes)
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
freezed-heading = Freezed
datatype-heading = Data type

pid-heading = Pid
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
search-type-label = Search Type:
unknown-search-description = Compare memory values without knowing the exact value

process-exited-title = Process has exited
process-exited-message = The target process is no longer running. Please return to the main menu to select a new process.
back-to-main-button = Back to Main Menu

# Memory editor
memory-editor-title = Memory Editor
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
