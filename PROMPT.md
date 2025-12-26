I want you to create a fully featured wrapper create for the librist library (RIST protocol for sending video).
You should make it in Rust 1.92 (latest version).
It should be be production-ready, follow best Rust practices, and be usable on multiple CPU arches (x64, ARM etc) and systems.
We should also have a full CI pipeline.

In the future i want to use this to create both servers and clients using the Rist Protocol.

Inspiration can be found in inspiration/ folder:

- librist - the full librist C library with my custom modifications. THIS IS YOUR SOURCE OF TRUTH
- rist-swift-wrapper - incomplete swift-wrapper used by Moblin app. Could be very useful.
- moblin - iOS app written in swift that uses the rist-swift-wrapper. Has both a RistServer and RistClient
- stream-relay - some random code i found, might be useful, has some librist stuff in Rust

Start planning it out, write plans to plans/. When in doubt always look at librist, for extra help and suggestions for implementations check the other inspirations.
