# RustFS

A Distributed File System implemented in Rust.

## Overview

RustFS is a simple distributed file system built using Rust and gRPC. It allows clients to upload, retrieve, update, and delete files on a remote server. This project demonstrates the use of Rust's asynchronous programming capabilities and gRPC for client-server communication.

## Features

- **Upload**: Stream files from the client to the server.
- **Get**: Retrieve the content of files stored on the server.
- **Append**: Append to the end of existing files on the server.
- **Delete**: Remove files from the server.

## Setup

1. Compile and run master and chunkservers

   ```bash
   # Compile project
   cargo build --release
   # Run master
   target/release/master -a localhost:50001
   target/release/master -a localhost:50002
   target/release/master -a localhost:50003
   # Run chunkserver
   target/release/chunkserver -a localhost:50010
   target/release/chunkserver -a localhost:50011
   ...
   ```

2. Sample client command
   ```bash
   # Upload file
   target/release/client upload <file_name>
   # Read file
   target/release/client read <file_name>
   # Delete file
   target/release/client delete <file_name>
   # Append file
   target/release/client append <file_name> <data>
   ```
