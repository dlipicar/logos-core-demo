cmake_minimum_required(VERSION 3.16)
project(bc_watch CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(logos-cpp-sdk REQUIRED)
find_package(nlohmann_json REQUIRED)

# liblogos and the plain protocol image it links, from the runtime this app
# spawns: one image, so the shell's credential is visible to the clients.
set(LOGOS_RUNTIME_LIB_DIR "" CACHE PATH "The runtime's lib/ (liblogos_core, liblogos_protocol_plain)")
set(LOGOS_PROTOCOL_INCLUDE_DIR "" CACHE PATH "logos-protocol's headers (logos_protocol.h)")
find_library(LOGOS_CORE_LIB logos_core PATHS "${LOGOS_RUNTIME_LIB_DIR}" NO_DEFAULT_PATH REQUIRED)
find_library(LOGOS_PROTOCOL_PLAIN_LIB logos_protocol_plain PATHS "${LOGOS_RUNTIME_LIB_DIR}" NO_DEFAULT_PATH REQUIRED)

add_executable(bc-watch main.cpp)
logos_generate_clients(TARGET bc-watch
    LIDL ${CMAKE_CURRENT_SOURCE_DIR}/../../contracts/peering_module.lidl
         ${CMAKE_CURRENT_SOURCE_DIR}/../../modules/bc_probe/contracts/blockchain_module.lidl
    TYPED_COLLECTIONS)
target_include_directories(bc-watch PRIVATE "${LOGOS_PROTOCOL_INCLUDE_DIR}")
target_link_libraries(bc-watch PRIVATE logos-cpp-sdk::logos_host nlohmann_json::nlohmann_json
                      "${LOGOS_CORE_LIB}" "${LOGOS_PROTOCOL_PLAIN_LIB}")
set_target_properties(bc-watch PROPERTIES BUILD_RPATH "${LOGOS_RUNTIME_LIB_DIR}"
                                          INSTALL_RPATH "${LOGOS_RUNTIME_LIB_DIR}")
install(TARGETS bc-watch RUNTIME DESTINATION bin)
