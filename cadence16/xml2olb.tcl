# xml2olb.tcl -- bulk-convert normalized Library XMLs to .olb inside a
# Capture 16.6 session. XMATIC_XML2OLB is Capture's internal XML->OLB
# entry point; there is no public CLI for it on 16.6, so this script is
# sourced in Capture's Tcl command window.
#
# Usage: with the CWD set to the directory holding the normalized *.xml
# files (run from that directory, or edit DIR below):
#     source xml2olb.tcl
# Writes xml2olb_result.txt: one "<stem> rc=<0|1> msg=<...>" per file,
# and reports which .olb files were created/updated.

set DIR [pwd]
set fp [open "xml2olb_result.txt" w]
set created {}
foreach x [glob -nocomplain [file join $DIR *.xml]] {
    set stem [file rootname [file tail $x]]
    if {$stem eq "xml2olb"} {continue}
    set dst [file join $DIR "$stem.olb"]
    if {[file exists $dst]} {set m 1} else {set m 0}
    set r [catch {XMATIC_XML2OLB $x $dst $m} msg]
    lappend created "$stem rc=$r msg=$msg"
}
foreach c $created {puts $fp $c}
close $fp
puts "xml2olb done -- see xml2olb_result.txt"
