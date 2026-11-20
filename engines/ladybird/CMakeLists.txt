cmake_minimum_required(VERSION 3.16)
project(kakangkung_ladybird_host LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

option(HAVE_LADYBIRD "Link against a Ladybird / LibWebView build" OFF)
set(LADYBIRD_SOURCE_DIR "" CACHE PATH "Path to a Ladybird checkout (required when HAVE_LADYBIRD=ON)")
set(LADYBIRD_BUILD_DIR "" CACHE PATH "Path to a Ladybird build directory (required when HAVE_LADYBIRD=ON)")

add_executable(kakangkung-ladybird-host
    src/host_main.cpp
    src/application.cpp
    src/stub_page.cpp
    src/abi.cpp
)

target_include_directories(kakangkung-ladybird-host PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${CMAKE_CURRENT_SOURCE_DIR}/src
)

target_compile_definitions(kakangkung-ladybird-host PRIVATE KAKANGKUNG_LADYBIRD_BUILD)

if(HAVE_LADYBIRD)
    if(NOT LADYBIRD_SOURCE_DIR OR NOT LADYBIRD_BUILD_DIR)
        message(FATAL_ERROR "HAVE_LADYBIRD=ON requires LADYBIRD_SOURCE_DIR and LADYBIRD_BUILD_DIR")
    endif()
    target_compile_definitions(kakangkung-ladybird-host PRIVATE HAVE_LADYBIRD=1)
    target_include_directories(kakangkung-ladybird-host PRIVATE
        ${LADYBIRD_SOURCE_DIR}
        ${LADYBIRD_SOURCE_DIR}/Userland/Libraries
        ${LADYBIRD_BUILD_DIR}
    )
    # Consumers must pass the Ladybird library set for their platform, e.g.:
    #   -DHAVE_LADYBIRD=ON -DLADYBIRD_SOURCE_DIR=... -DLADYBIRD_BUILD_DIR=...
    #   and link LibWebView + dependencies via LADYBIRD_LIBRARIES.
    if(DEFINED LADYBIRD_LIBRARIES)
        target_link_libraries(kakangkung-ladybird-host PRIVATE ${LADYBIRD_LIBRARIES})
    else()
        message(WARNING "HAVE_LADYBIRD=ON but LADYBIRD_LIBRARIES is empty; stub sources still compile. Add view_ladybird.cpp and link LibWebView to finish the UI port.")
    endif()
endif()

install(TARGETS kakangkung-ladybird-host RUNTIME DESTINATION bin)
