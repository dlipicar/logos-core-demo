cmake_minimum_required(VERSION 3.14)
project(bc_probe_plugin LANGUAGES CXX)

if(DEFINED ENV{LOGOS_MODULE_BUILDER_ROOT})
    include($ENV{LOGOS_MODULE_BUILDER_ROOT}/cmake/LogosModule.cmake)
else()
    message(FATAL_ERROR "LogosModule.cmake not found: build with nix (logos-module-builder)")
endif()

logos_module(
    NAME bc_probe
    SOURCES
        src/bc_probe_impl.h
        src/bc_probe_impl.cpp
)
