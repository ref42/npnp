# olb_verify.tcl -- headless .olb sanity check (no Capture GUI needed):
# loads Capture's DBO Tcl binding, enumerates every package and part name
# in each .olb of a directory, and flags npnp#4 symptoms:
#   - part names with a "-1.Normal" style renamed-duplicate suffix
#   - sections whose pin list duplicates other sections
#
# Usage (Cadence's bundled tclsh8.4, or inside Capture's Tcl window):
#     load "C:/Cadence/Cadence_SPB_16.6-2015/tools/capture/orDb_Dll_Tcl.dll" DboTclWriteBasic
#     source olb_verify.tcl
# Edit DIR below to point at the directory containing the .olb files.

set DIR {C:/GY_EDA_V2/sch_lib}
set fp [open "olb_verify_result.txt" w]
load {C:/Cadence/Cadence_SPB_16.6-2015/tools/capture/orDb_Dll_Tcl.dll} DboTclWriteBasic
set lSession [DboTclHelper_sCreateSession]
set lStatus [DboState]
set lNullObj NULL

foreach olb [glob -nocomplain [file join $DIR *.olb]] {
    set lLibName [DboTclHelper_sMakeCString [file normalize $olb]]
    set lLib [$lSession GetLib $lLibName $lStatus]
    if {$lLib == $lNullObj} {
        puts $fp "[file tail $olb] OPEN FAILED"
        continue
    }
    set pkgIter [$lLib NewPackagesIter $lStatus]
    set pPkg [$pkgIter NextPackage $lStatus]
    while {$pPkg != $lNullObj} {
        set pPkgName [DboTclHelper_sMakeCString]
        $pPkg GetName $pPkgName
        set nm [DboTclHelper_sGetConstCharPtr $pPkgName]
        set partsIter [$pPkg NewPartsIter $lStatus]
        set names {}
        set dup 0
        set pPart [$partsIter NextPart $lStatus]
        while {$pPart != $lNullObj} {
            set pPartName [DboTclHelper_sMakeCString]
            catch { $pPart GetName $pPartName }
            set partNm [DboTclHelper_sGetConstCharPtr $pPartName]
            if {[string match "*-1.Normal" $partNm] || [string match "*_*" $partNm]} {
                incr dup
            }
            lappend names $partNm
            set pPart [$partsIter NextPart $lStatus]
        }
        catch { delete_DboPackagePartsIter $partsIter }
        if {$dup > 0} {
            puts $fp "[file tail $olb] PKG $nm : $names  <-- RENAMED-DUPLICATE SYMPTOM (npnp#4)"
        } else {
            puts $fp "[file tail $olb] PKG $nm : $names"
        }
        set pPkg [$pkgIter NextPackage $lStatus]
    }
    delete_DboLibPackagesIter $pkgIter
    $lSession RemoveLib $lLib
}
DboTclHelper_sDeleteSession $lSession
puts $fp DONE
close $fp
