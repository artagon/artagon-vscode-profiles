# Example toolchain file for Homebrew LLVM. Copy/modify per project.
set(CMAKE_C_COMPILER "/opt/homebrew/opt/llvm/bin/clang" CACHE FILEPATH "")
set(CMAKE_CXX_COMPILER "/opt/homebrew/opt/llvm/bin/clang++" CACHE FILEPATH "")
set(CMAKE_LINKER "/opt/homebrew/opt/llvm/bin/ld" CACHE FILEPATH "")
