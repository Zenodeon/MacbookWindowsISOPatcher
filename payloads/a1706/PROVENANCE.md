# A1706 payload provenance

The startup script, Iris state parser, dialog logger, audio cue, winpeshl.ini, and
SetupComplete.cmd were copied from the v17 staging files in
`C:/Github/MacbookISOBuildingSpace/work/staging` on 2026-10-19.
The runtime script retains the source sequence and is stored as ASCII with CRLF.
The original hardcoded GPU instance assignment is replaced by a call to
`a1706gpu.exe`, which resolves the present device by the documented PCI prefix.
`a1706gpu.c` is new. Helper executables are compiled from these sources by build.rs.

The reproduction guide is the authoritative behavior reference. Do not import
the earlier unsigned-INF, live-hive, or stock-boot-image experiments.
