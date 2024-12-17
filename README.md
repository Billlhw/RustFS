# Distributed File System: RustFS

## Team Information 
- **Yiren Zhao** (1005092427): yiren.zhao@mail.utoronto.ca
- **Haowei Li** (1004793565): haowei.li@mail.utoronto.ca

## Video Demo
https://drive.google.com/file/d/1UHPI_3khvqXPu96EOfEeS2U-vYIC1_0R/view?usp=sharing

## 1. Motivation
Distributed file systems are the backbone of modern data-driven applications, offering scalability, reliability, and high availability. With Rust's rise as a systems programming language, its ecosystem has expanded to include high-performance and resilient storage systems.

While the Google File System (GFS) has inspired numerous distributed file systems, it is not open source, and critical aspects of its design, such as node discovery for the master and load balancing algorithms, are not explicitly documented. Moreover, existing implementations of GFS are insufficient for real-world use cases. For instance, the Python-based implementation in [1] fails to separate files into chunks, violating the core principle of chunkservers. Similarly, the Rust-based implementation rdfs [2] provides only a toy model of a file system, lacking fundamental details needed for a robust and scalable distributed file system.

One of the key advantages of chunk-based storage, as used in GFS, is its ability to divide large files into smaller, fixed-size chunks. This approach allows for efficient data distribution across multiple nodes, enhancing scalability and parallelism. Additionally, chunk-based storage facilitates fault tolerance by enabling the replication of individual chunks across different nodes, ensuring data availability even in the event of node failures.

Our project aims to bridge this gap by leveraging Rust's concurrency and memory safety features to develop a distributed file system that adheres to the principles of GFS. All components of our system are fault-tolerant, ensuring reliability and availability in distributed environments. Our implementation includes chunk-based storage, master node fault recovery, and efficient load balancing, all inspired by GFS, making it a performant and highly available solution.

This project provided us with an opportunity to explore distributed systems concepts and address a real-world need for a complete, reliable, and user-friendly distributed file system in the Rust ecosystem. By contributing a robust implementation, we aim to fill the existing gap and provide a foundation for future enhancements in the distributed storage domain.

## 2. Objectives
The objective of our project is to develop a scalable, high-available, and high-performance file storage system. Our system aims to support growing data and user demands by allowing on-demand addition of storage nodes without disrupting existing operations. To ensure high availability and durability, the system is resilient to component failures and supports automatic detection and recovery of node failures. Efficiency is further optimized through client-side metadata caching, and planning of dataflow during mutations. Additionally, we will incorporate advanced functionality, including access control, end-to-end encryption, and file tailing.

## 3. Key Features
In this section, we introduce the key features of the system. Figure 1 presents a component diagram and an illustration of the workflow for a read operation.
![RustFS_Architecture_v3](https://github.com/user-attachments/assets/d18df6a1-7d04-4fa6-a25c-14786d0191fd)
*Figure 1: Component Diagram*

We adopt a centralized design in which the master node holds and manages metadata. The master node is responsible for assigning chunks to chunkservers, monitoring the liveliness of each chunkserver, and rebalancing load across chunkservers to ensure the availability of file chunks and the read performance of the system. Additionally, this design improves the maintainability of the system and simplifies the implementation of authentication.

In GFS, centralized management of metadata has the added benefit of ensuring strong consistency by assigning a primary node for each chunk and having the primary node assign a total mutation order for the chunk, which is followed on all other replicas. This is a future step for our system.

### 3.1 Load Balancing
The load balancing algorithm is not explicitly described in the GFS paper. We illustrate an implementation here that can be used as a reference for relevant applications.

Figure 2 provides an example of the distribution of chunks across chunkservers. Each file can be divided into a different number of chunks, depending on its size. Each chunk has the same number of replicas, and we allow the replication factor to be configurable. Notably, in the system, the distribution of replicas on chunkservers is balanced, and no single chunkserver stores multiple replicas of the same chunk, which adheres to the principle of replication for improved fault tolerance.
![Chunk_Diagram_v2](https://github.com/user-attachments/assets/fd8036bf-a3f2-4a83-8b68-f64fe6f44f6a)
*Figure 2: Distribution of Chunks on Chunkservers*

When a new chunk needs to be assigned, the master identifies all available nodes, meaning nodes with a load less than max_allowed_chunks, which is a configurable parameter that manages the maximum amount of data each chunkserver can handle. The algorithm then iteratively selects available chunkservers for all replicas, prioritizing those with the minimal load.

When load rebalancing is required due to a chunkserver crash, the master follows a similar process of selecting available nodes and excluding those already storing replicas of the same chunk. After selecting the new chunkservers, the master instructs an available chunkserver to send the chunk to the newly selected node. For instance, in Figure 2, if Chunkserver 3 crashes, File_1_Chunk_1 will be migrated to Chunkserver 2, and File_2_Chunk_1 will be migrated to Chunkserver 4, as these are the only available chunkservers for the two failed chunks.

### 3.2 Fault Tolerance
Achieving fault tolerance across all components is essential for scalability, as it enables the system to handle increasing loads without introducing single points of failure. Additionally, fault-tolerant components enhance availability, ensuring the system remains operational and responsive despite hardware failures, network disruptions, or software crashes. Since the client node is stateless and does not require recovery, we focus on the fault tolerance mechanisms for the master and chunkserver nodes.

#### 3.2.1 Fault Tolerance of the Master Node
Fault tolerance of the master node in GFS involves using external servers to detect the availability of the master node and a DNS server for discovering shadow masters, which serve as backups for the master node. We implement a simplified design that provides fault tolerance without involving external servers, while ensuring a seamless switch to backup nodes.

In the configuration file, we store a list of addresses for all master nodes. The first master node that starts takes on the role of the active master. This is enforced by requiring each master node to ping all other master addresses before assuming the role. If another node responds, indicating it is the currently active master, the newly started master becomes a shadow master.

During normal operations, only the master node is responsible for updating metadata to ensure consistency. Metadata updates are propagated to the shadow masters in real time to ensure that, if the current master crashes, the node that takes over has up-to-date data.

Additionally, shadow masters are configured to ping the master node periodically. If the master node cannot be reached, a shadow master assumes the master role. Once the original master recovers, it becomes a shadow master. This approach works well for a total of two master nodes. For more nodes, there is a risk that multiple shadow masters may concurrently assume the master role. This issue can be resolved by implementing a global ordering of master nodes, with the shadow master of the highest priority taking the master role first. Alternatively, a Rust-based leader election algorithm could be used. This is left as future work.

#### 3.2.2 Fault Tolerance of the Chunkservers
The liveliness of chunkservers is monitored by the master node. Chunkservers send heartbeats to the master node, which periodically checks the latest heartbeat from each chunkserver. If the interval since the last heartbeat exceeds a configurable threshold, the master assumes the chunkserver is down, removes its chunks from metadata, and uses the load rebalancing algorithm introduced in Section 3.1 to reassign the failed chunks.

Write operations are impacted only for the duration of the interval between the master’s periodic checks, which is configurable. Read operations, however, are not suspended during this period because the client selects a random server to read from and retries with another server if the selected one has failed.

### 3.3 User Authentication
User authentication is crucial in a distributed file system to ensure that only authorized users can access, modify, or delete files, by verifying user identities before granting permissions. This safeguards the system against malicious activities, data breaches, and ensures accountability for file operations.

In our system, the master node maintains a list of valid usernames and passwords locally, with the file path being configurable. When user authentication is enabled, the client sends the username and password to the master node for verification. Upon successful authentication, the master generates a One-Time Password (OTP) and distributes it to both the user and all chunkservers. For subsequent file read or modification requests, the client includes the OTP, which the chunkservers verify before granting access.

This design enhances security by ensuring that only authenticated users with valid credentials can access or modify files, while the OTP prevents credential replay attacks by being valid for a limited duration. Additionally, distributing the OTP to chunkservers ensures that authentication verification can occur without frequent communication with the master, improving system performance and scalability.

### 3.4 Command-Line Interface and Configurability
Our system provides a standard file system interface to read, upload, append, and delete files, enabling clients to perform essential operations on files stored in GFS while ensuring user-friendliness. Details about the command formats are provided in Section 5.

Additionally, the designed system is highly configurable. Clients can customize parameters such as the replication factor of each chunk, the maximum number of chunks per chunkserver, the heartbeat interval between chunkservers and the master, and the interval between shadow masters and the master. This flexibility allows clients to tailor the system to their specific needs.

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
    target/release/master -a 127.0.0.1:50001
    target/release/master -a 127.0.0.1:50002
    target/release/master -a 127.0.0.1:50003
    ```
    Verify the output in each terminal. One of the nodes will elect itself as the leader.

### Step 2: Start Chunkservers
Start chunkservers that will store file data. Run each chunkserver in separate terminals:
```
target/release/chunkserver -a 127.0.0.1:50010
target/release/chunkserver -a 127.0.0.1:50011
```
Ensure the `data_path` directory exists for storing chunk data:
```
mkdir -p data
```
Verify chunkserver logs to ensure they successfully register with the master node.

### Step 3: Test the System Using the Client

Once the master nodes and chunkservers are running, use the client to perform file operations. Basic operations including uploading, reading, appending, and deleting files. The following examples demonstrate these operations using ```example.txt``` as the ```<file_name>```:

#### Command 1: Upload a File

Upload a file to the distributed file system:

```
target/release/client upload <file_name>
```

Expected output:
```
Uploading <file_name>...
File successfully uploaded.
```


#### Command 2: Read a File
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

#### Command 3: Append to a File
Append data to the end of an existing file:

```
target/release/client append <file_name> "<data>"
```
Expected output:
```
Appending data to <file_name>...
Append successful.
```

#### Command 4: Delete a File
Delete a file from the system:
```
target/release/client delete <file_name>
```
Expected output:
```
Deleting <file_name>...
File successfully deleted.
```

### Authentication Feature
To use this feature, modify the value of `use_authentication` in the `config.toml` file:
```
use_authentication = false
```
After updating the configuration, restart both the master and chunkserver processes to apply the changes.
Use the -u and -p flags to provide the username and password, respectively:
```
target/release/client upload example.txt -u user1 -p password1
target/release/client read example.txt -u user1 -p password1
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
  - Developed the user authentication feature.

## 7. Lessons Learned and Concluding Remarks

### Lessons Learned
Throughout this project, we gained invaluable insights into the complexities of distributed systems, particularly in ensuring fault tolerance and consistency across distributed environments. Working with Rust provided a deeper understanding of memory safety and concurrency, and familiarized us with the modern language's distinct design philosophy.

One significant takeaway is that the development lifecycle differed significantly from our experience with other languages like C++. Fixing syntax and ownership-related issues consumed far more time than debugging runtime errors—a stark contrast to languages that require manual memory management. This shift highlighted the need for a development plan that allocates more time to the coding phase and anticipates a steeper learning curve. That said, the trade-off is fewer runtime errors and more predictable behavior, which we found highly valuable.

Another observation is that, although Rust can eliminate the majority of concurrency problems, deadlock issues can still persist. However, these are among the easiest concurrency issues to debug. We conclude that avoiding "hold and wait" conditions still requires careful attention during programming.

Next, we appreciate the importance of organizing code for modularity and extensibility. Our final deliverable adopted a modular architecture, enabling us to add features with minimal impact on existing functionality. However, this modular design came at the cost of refactoring for clarity and maintainability, which became necessary when we noticed that the code for the master node and chunk server readily exceeded 1,000 lines before implementing some of the core features. Our takeaway is that breaking down such large functionalities into manageable modules in Rust requires careful planning from the start.

Another critical lesson was the importance of thoroughly designing features prior to implementation. For example, designing the master coordination algorithm, including leader selection and the mechanism to keep shadow masters up to date, required careful consideration, as we aimed for a simplistic design that does not rely on external servers. A clear and well-documented plan significantly expedited the development.


### Concluding Remarks
In conclusion, RustFS ensures efficient handling of concurrent operations, robust fault tolerance, and secure user authentication, which are critical for large-scale file systems. Looking ahead, we believe this project lays a strong foundation for further exploration of distributed file systems in Rust. The system could potentially serve as a base for more advanced research or commercial applications. Future work might include implementing mechanisms to ensure strong consistency, integrating machine learning for predictive load balancing, and extending the system to support object storage for cloud environments.

## 8. References
[1] "Google File System," GitHub repository, Available: https://github.com/chaitanya100100/Google-File-System/tree/master/src. [Accessed: Dec. 15, 2024].

[2] "rdfs: A Rust-based distributed file system," GitHub repository, Available: https://github.com/watthedoodle/rdfs. [Accessed: Dec. 15, 2024].

## **License**
This project is licensed under the MIT License. See the LICENSE file for more details.
