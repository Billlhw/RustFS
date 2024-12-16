# Distributed File System: RustFS

## Team Information 
- **Yiren Zhao** (1005092427): yiren.zhao@mail.utoronto.ca
- **Haowei Li** (1004793565): haowei.li@mail.utoronto.ca


## 1. Motivation
Distributed file systems are the backbone of modern data-driven applications, offering scalability, reliability, and high availability. With Rust's rise as a systems programming language, its ecosystem has expanded to include high-performance and resilient storage systems.

While the Google File System (GFS) has inspired numerous distributed file systems, it is not open source, and critical aspects of its design, such as node discovery for the master and load balancing algorithms, are not explicitly documented. Moreover, existing implementations of GFS are insufficient for real-world use cases. For instance, the Python-based implementation in [1] fails to separate files into chunks, violating the core principle of chunkservers. Similarly, the Rust-based implementation rdfs [2] provides only a toy model of a file system, lacking fundamental details needed for a robust and scalable distributed file system.

One of the key advantages of chunk-based storage, as used in GFS, is its ability to divide large files into smaller, fixed-size chunks. This approach allows for efficient data distribution across multiple nodes, enhancing scalability and parallelism. Additionally, chunk-based storage facilitates fault tolerance by enabling the replication of individual chunks across different nodes, ensuring data availability even in the event of node failures.

Our project aims to bridge this gap by leveraging Rust's concurrency and memory safety features to develop a distributed file system that adheres to the principles of GFS. All components of our system are fault-tolerant, ensuring reliability and availability in distributed environments. Our implementation includes chunk-based storage, master node fault recovery, and efficient load balancing, all inspired by GFS, making it a performant and highly available solution.

This project provided us with an opportunity to explore distributed systems concepts and address a real-world need for a complete, reliable, and user-friendly distributed file system in the Rust ecosystem. By contributing a robust implementation, we aim to fill the existing gap and provide a foundation for future enhancements in the distributed storage domain.

## 2. Objectives
The objective of our project is to develop a scalable, high-available, and high-performance file storage system. Our system aims to support growing data and user demands by allowing on-demand addition of storage nodes without disrupting existing operations. To ensure high availability and durability, the system is resilient to component failures and supports automatic detection and recovery of node failures. Efficiency is further optimized through client-side metadata caching, and planning of dataflow during mutations. Additionally, we will incorporate advanced functionality, including access control, end-to-end encryption, and file tailing.


## 3. Features
System Architecture
![Component_Diagram (1)](https://github.com/user-attachments/assets/b6c7e9d8-0ce8-4420-b78f-84ddd63e108c)

### 3.1 Core Features
- **Centralized Metadata Management**: A master node maintains and replicates metadata, ensuring consistency and durability across the system.
- **Data Replication**: Files are replicated across multiple chunkservers to guarantee availability and fault tolerance.
- **Replica Consistency Management**: A lease mechanism ensures that mutations are applied in the same order across replicas.
- **Scalable Architecture**: The system supports horizontal scaling by adding new chunkservers without service disruption.

### 3.2 Advanced Features
- **End-to-End Encryption**: Secure data transmission using asymmetric encryption for key exchange and symmetric encryption for file chunks.
- **Real-Time File Tailing**: Enables real-time updates to append-only files, useful for log tracking.
- **User Authentication**: Validates client credentials and issues session tokens for secure communication.
- **Automated Failure Recovery**: Detects master and chunkserver failures and initiates rebalancing or leader election for recovery.

## 4. Reproducibility Guide
This guide explains how to clone, set up, build RustFS step-by-step, ensuring a working distributed file system on your local machine.

### 4.1 Prerequisites
Before proceeding, ensure the following requirements are met:

#### Operating System
- **Supported OS**: macOS Sonoma.

#### Rust Installation
RustFS requires the Rust programming language and its package manager, Cargo. Install Rust using the official [rustup installer](https://rustup.rs/):
```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Once installed, verify the installation:
```
rustc --version
cargo --version
```
Ensure Rust version is **1.70+**.

#### Hardware Requirements
At least 4GB RAM and 10GB storage.

### 4.2 Setting Up RustFS

**Step 1: Clone the Repository**

Clone the RustFS repository from GitHub to your local machine:
```
git clone https://github.com/Billlhw/RustFS.git
cd RustFS
```
    
**Step 2: Review the Configuration**

Ensure the `config.toml` file is properly set up. This file contains key settings for master nodes, chunkservers, and clients.

Here’s a sample `config.toml`
```
[master]
log_path = "logs"
cron_interval = 5
heartbeat_failure_threshold = 2

[chunkserver]
data_path = "data"
log_path = "logs"

[client]
log_path = "client/logs"

[common]
master_addrs = ["127.0.0.1:50001", "127.0.0.1:50002", "127.0.0.1:50003"]
heartbeat_interval = 10
chunk_size = 4096
max_allowed_chunks = 100
replication_factor = 2
log_level = "info"
log_output = "stdout"
```
Make sure the `master_addrs` lists all master nodes and that `data_path` is a writable directory for chunkservers.

**Step 3: Build the System**

Compile RustFS in release mode:
```
cargo build --release
```

This will generate the following binaries:

- `target/release/master`: Master node executable
- `target/release/chunkserver`: Chunkserver executable
- `target/release/client`: Client executable

Ensure these files exist:

```
ls target/release/master target/release/chunkserver target/release/client
```

### 4.3 Troubleshooting

**Issue: Compilation Errors**
- Ensure you have the latest Rust toolchain installed.
- Run cargo clean and recompile the project:
  ```
  cargo clean
  cargo build --release
  ```

**Issue: Chunkserver Not Registering**
- Verify the ```master_addrs``` in ```config.toml``` is correct.
- Check if the master node is running.

**Issue: Issue: File Operations Fail**
- Ensure all chunkservers and master nodes are running.
- Check logs for error messages.
  
      
## 5. Running RustFS Guide
You need to start the **master nodes**, **chunkservers**, and then use the **client** to interact with the system.

### Step 1: Start Master Nodes
    Start multiple master nodes (as part of leader-election and fault tolerance). Run each of the following commands in a separate terminal window:
    ```
    target/release/master -a localhost:50001
    target/release/master -a localhost:50002
    target/release/master -a localhost:50003
    ```
    Verify the output in each terminal. One of the nodes will elect itself as the leader.

### Step 2: Start Chunkservers
Start chunkservers that will store file data. Run each chunkserver in separate terminals:
```
target/release/chunkserver -a localhost:50010
target/release/chunkserver -a localhost:50011
```
Ensure the `data_path` directory exists for storing chunk data:
```
mkdir -p data
```
Verify chunkserver logs to ensure they successfully register with the master node.

### Step 3: Test the System Using the Client

Once the master nodes and chunkservers are running, use the client to perform file operations. Basic operations including uploading, reading, appending, and deleting files. The following examples demonstrate these operations using ```example.txt``` as the ```<file_name>```:

### Command 1: Upload a File

Upload a file to the distributed file system:

```
target/release/client upload <file_name>
```

Expected output:
```
Uploading <file_name>...
File successfully uploaded.
```


### Command 2: Read a File
Read the contents of a file stored in the system:
```
target/release/client read <file_name>
```
Expected output:
```
Reading <file_name>...
File contents:
[contents of <file_name>]
```

### Command 3: Append to a File
Append data to the end of an existing file:

```
target/release/client append <file_name> "<data>"
```
Expected output:
```
Appending data to <file_name>...
Append successful.
```

### Command 4: Delete a File
Delete a file from the system:
```
target/release/client delete <file_name>
```
Expected output:
```
Deleting <file_name>...
File successfully deleted.
```

## 6. Contributions by Team Members
- **Yiren Zhao**:
  - Designed and implemented the chunkserver logic, including data storage and replication mechanisms, ensuring efficient and reliable data handling across the system.
  - Engineered the fault detection and failure recovery mechanism for chunkservers.
  - Developed the leader selection algorithm for the master node, enabling seamless coordination of components in the distributed system.
  - Built and throughly tested the core file operations, including upload, read, append, and delete, ensuring the system's usability.

- **Haowei Li**:
  - Architected the master node logic, encompassing metadata management and load balancing to maintain system performance and consistency.
  - Designed and implemented metadata update propagation feature and master failure recovery mechanisms
  - Programmed the logic to split files into chunks for file operations, adhering to the core principles of chunk-based distributed storage.
  - Developed real-time file tailing feature.

## 7. Lessons Learned and Concluding Remarks

### Lessons Learned
Throughout this project, we gained invaluable insights into the complexities of distributed systems, particularly in ensuring fault tolerance and consistency across distributed environments. Working with Rust provided a deeper understanding of memory safety and concurrency, and familiarized us with the modern language's distinct design philosophy.

One significant takeaway is that the development lifecycle differed significantly from our experience with other languages like C++. Fixing syntax and ownership-related issues consumed far more time than debugging runtime errors—a stark contrast to languages that require manual memory management. This shift highlighted the need for a development plan that allocates more time to the coding phase and anticipates a steeper learning curve. That said, the trade-off is fewer runtime errors and more predictable behavior, which we found highly valuable.

Another observation is that, although Rust can eliminate the majority of concurrency problems, deadlock issues can still persist. However, these are among the easiest concurrency issues to debug. We conclude that avoiding "hold and wait" conditions still requires careful attention during programming.

Next, we appreciate the importance of organizing code for modularity and extensibility. Our final deliverable adopted a modular architecture, enabling us to add features with minimal impact on existing functionality. However, this modular design came at the cost of refactoring for clarity and maintainability, which became necessary when we noticed that the code for the master node and chunk server readily exceeded 1,000 lines before implementing some of the core features. Our takeaway is that breaking down such large functionalities into manageable modules in Rust requires careful planning from the start.

Another critical lesson was the importance of thoroughly designing features prior to implementation. For example, designing the master coordination algorithm, including leader selection and the mechanism to keep shadow masters up to date, required careful consideration, as we aimed for a simplistic design that does not rely on external servers. A clear and well-documented plan significantly expedited the development.

Looking ahead, we believe this project lays a strong foundation for further exploration of distributed file systems in Rust. The system could potentially serve as a base for more advanced research or commercial applications. Future work might include implementing mechanisms to ensure strong consistency, integrating machine learning for predictive load balancing, and extending the system to support object storage for cloud environments.

### Concluding Remarks
This project successfully combines core file storage functionalities with advanced features like encryption, file tailing, and fault tolerance. The system is scalable, secure, and resilient to failures. Future work could include optimizing the metadata handling for large-scale deployments and adding support for cross-rack replication.

## 8. References
[1] "Google File System," GitHub repository, Available: https://github.com/chaitanya100100/Google-File-System/tree/master/src. [Accessed: Dec. 15, 2024].

[2] "rdfs: A Rust-based distributed file system," GitHub repository, Available: https://github.com/watthedoodle/rdfs. [Accessed: Dec. 15, 2024].

## **License**
This project is licensed under the MIT License. See the LICENSE file for more details.
