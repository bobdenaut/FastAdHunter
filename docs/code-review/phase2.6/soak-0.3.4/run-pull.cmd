@echo off
rem Hourly soak pull for 0.3.4. The API key lives outside the repo in
rem %USERPROFILE%\.fah-soak\token.txt so nothing secret is ever committed.
setlocal
set SOAK_DIR=%~dp0
for /f "usebackq delims=" %%K in ("%USERPROFILE%\.fah-soak\token.txt") do set FAH_TOKEN=%%K
python "%SOAK_DIR%collect-soak.py" ^
  --base https://fah-api.localbox.ro:8443 ^
  --token %FAH_TOKEN% ^
  --out "%SOAK_DIR%pulls" ^
  --ssh-host bobdenaut >> "%SOAK_DIR%collector.log" 2>&1
endlocal
