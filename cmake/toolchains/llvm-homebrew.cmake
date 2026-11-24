# Example toolchain file for Homebrew LLVM on Apple Silicon macOS (CMake 3.29+).
# Copy/modify per project. For Linux, set LLVM_PREFIX to your LLVM prefix (e.g., /usr).
set(LLVM_PREFIX "/opt/homebrew/opt/llvm" CACHE PATH "Homebrew LLVM prefix")

set(CMAKE_C_COMPILER "${LLVM_PREFIX}/bin/clang" CACHE FILEPATH "")
set(CMAKE_CXX_COMPILER "${LLVM_PREFIX}/bin/clang++" CACHE FILEPATH "")
set(CMAKE_AR "${LLVM_PREFIX}/bin/llvm-ar" CACHE FILEPATH "")
set(CMAKE_RANLIB "${LLVM_PREFIX}/bin/llvm-ranlib" CACHE FILEPATH "")

# Use Homebrew LLD via explicit absolute path only if installed (`brew install lld`), so GUI
# CMake sessions work even when /opt/homebrew/bin is off PATH; otherwise use Apple ld-prime.
find_program(_HOMEBREW_LLD NAMES ld64.lld ld.lld HINTS "/opt/homebrew/opt/lld/bin" "${LLVM_PREFIX}/bin")
if(_HOMEBREW_LLD)
  string(APPEND CMAKE_EXE_LINKER_FLAGS_INIT " -fuse-ld=${_HOMEBREW_LLD}")
  string(APPEND CMAKE_SHARED_LINKER_FLAGS_INIT " -fuse-ld=${_HOMEBREW_LLD}")
  string(APPEND CMAKE_MODULE_LINKER_FLAGS_INIT " -fuse-ld=${_HOMEBREW_LLD}")
else()
  set(CMAKE_LINKER_TYPE "SYSTEM" CACHE STRING "")
endif()

# Ensure Homebrew LLVM libc++/libc++abi are linked and discoverable at runtime on macOS.
if(APPLE AND EXISTS "${LLVM_PREFIX}/lib/c++")
  string(APPEND CMAKE_EXE_LINKER_FLAGS_INIT " -L${LLVM_PREFIX}/lib/c++ -Wl,-rpath,${LLVM_PREFIX}/lib/c++")
  string(APPEND CMAKE_SHARED_LINKER_FLAGS_INIT " -L${LLVM_PREFIX}/lib/c++ -Wl,-rpath,${LLVM_PREFIX}/lib/c++")
  string(APPEND CMAKE_MODULE_LINKER_FLAGS_INIT " -L${LLVM_PREFIX}/lib/c++ -Wl,-rpath,${LLVM_PREFIX}/lib/c++")
endif()
