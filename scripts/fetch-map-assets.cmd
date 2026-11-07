@echo off
rem Thin Windows wrapper for the cross-platform Node asset fetcher.
setlocal
node "%~dp0fetch-map-assets.mjs" %*
exit /b %errorlevel%
