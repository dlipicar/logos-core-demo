cmake_minimum_required(VERSION 3.14)
project(hello_module_plugin LANGUAGES CXX)

if(DEFINED ENV{LOGOS_MODULE_BUILDER_ROOT})
    include($ENV{LOGOS_MODULE_BUILDER_ROOT}/cmake/LogosModule.cmake)
else()
    message(FATAL_ERROR "LogosModule.cmake not found: build with nix (logos-module-builder)")
endif()

logos_module(
    NAME hello_module
    SOURCES
        src/hello_module_impl.h
        src/hello_module_impl.cpp
)
