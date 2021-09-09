
# ---
# ockam::ffi_interface
# ---
add_library(ockam_ffi_interface INTERFACE)
add_library(ockam::ffi_interface ALIAS ockam_ffi_interface)

set(INCLUDE_DIR ${CMAKE_CURRENT_BINARY_DIR}/include)

target_include_directories(ockam_ffi_interface INTERFACE ${INCLUDE_DIR})

file(COPY ../../../../../../rust/ockam/ockam_ffi/include/vault.h DESTINATION ${INCLUDE_DIR}/ockam)

target_sources(
  ockam_ffi_interface
  INTERFACE
    ${INCLUDE_DIR}/ockam/vault.h
)

add_library(ockam_ffi SHARED IMPORTED GLOBAL)
add_library(ockam::ffi ALIAS ockam_ffi)

# SOMEDAY: We should determine the rust target triple, and also check:
# - `target/${rust_target_triple}/debug`
# - `$ENV{CARGO_TARGET_DIR}/target/debug` if `CARGO_TARGET_DIR` is set in the env
# - `$ENV{CARGO_TARGET_DIR}/${rust_target_triple}/target/debug` if `CARGO_TARGET_DIR` is set in the env

file(GLOB
        FFI_LIB_PATH
        ${CMAKE_CURRENT_LIST_DIR}/../../../../../../../target/debug/libockam_ffi${CMAKE_SHARED_LIBRARY_SUFFIX}
        ${CMAKE_CURRENT_LIST_DIR}/../../../../../../../target/release/libockam_ffi${CMAKE_SHARED_LIBRARY_SUFFIX})

file(COPY ${FFI_LIB_PATH} DESTINATION "${CMAKE_CURRENT_BINARY_DIR}")

set(FFI_LIB_PATH "${CMAKE_CURRENT_BINARY_DIR}/libockam_ffi${CMAKE_SHARED_LIBRARY_SUFFIX}")

get_filename_component(FFI_LIB_PATH "${FFI_LIB_PATH}" REALPATH BASE_DIR "${CMAKE_CURRENT_BINARY_DIR}")
set_target_properties(
        ockam_ffi
        PROPERTIES
        IMPORTED_LOCATION "${FFI_LIB_PATH}"
)

add_library(ockam_elixir_ffi)
add_library(ockam::elixir_ffi ALIAS ockam_elixir_ffi)

target_sources(ockam_elixir_ffi PRIVATE nifs.c vault.c vault.h common.c common.h)

target_include_directories(ockam_elixir_ffi PUBLIC $ENV{ERL_INCLUDE_DIR})

if(APPLE)
set_target_properties(ockam_elixir_ffi PROPERTIES LINK_FLAGS "-dynamiclib -undefined dynamic_lookup")
endif()

if(UNIX AND NOT APPLE)
set_target_properties(ockam_elixir_ffi PROPERTIES LINK_FLAGS "-fPIC -shared")
endif()

target_link_libraries(
        ockam_elixir_ffi
  PUBLIC
    ockam::ffi_interface
    ockam::ffi
)
