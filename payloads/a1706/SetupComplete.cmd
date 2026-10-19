@echo off
pnputil /add-driver %SystemDrive%\Drivers\*.inf /subdirs /install
