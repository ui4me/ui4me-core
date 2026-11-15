@echo off
cd /d F:\agent-workspaces\ui4me-core\stab-374b06f6\src
set "PY=F:\agent-workspaces\ui4me-core\stab-374b06f6\py\cpython-3.13-windows-x86_64-none"
set "PYTHON3=%PY%\python.exe"
set "PATH=%PY%;%PATH%"
set "CARGO_TARGET_DIR=F:\agent-workspaces\ui4me-core\stab-374b06f6\target"
echo === build host
cargo build --release --locked -p ui4me-host -j 3 2>&1 | findstr /R /C:"Finished" /C:"error"
echo === pe stack
"%PYTHON3%" docs\audit\evidence\scripts\pe_stack.py %CARGO_TARGET_DIR%\release\ui4me-host.exe
echo === host tests
cargo test --release --locked -p ui4me-host -j 3 2>&1 | findstr /R /C:"test result" /C:"FAILED" /C:"panicked"
echo === stack_depth
cargo test --release --locked -p ui4me-renderer --test stack_depth -j 3 2>&1 | findstr /R /C:"test result" /C:"FAILED" /C:"panicked" /C:"overflow"
echo === content_stalls
cargo test --release --locked -p ui4me-renderer --test content_stalls -j 3 2>&1 | findstr /R /C:"test result" /C:"FAILED" /C:"panicked"
echo === svg_filter_cost
cargo test --release --locked -p ui4me-assets --test svg_filter_cost -j 3 2>&1 | findstr /R /C:"test result" /C:"FAILED" /C:"panicked"
echo === svg_depth
cargo test --release --locked -p ui4me-assets --test svg_depth -j 3 2>&1 | findstr /R /C:"test result" /C:"FAILED" /C:"panicked"
echo === renderer lib
cargo test --release --locked -p ui4me-renderer --lib -j 3 2>&1 | findstr /R /C:"test result" /C:"FAILED" /C:"panicked"
echo === done
