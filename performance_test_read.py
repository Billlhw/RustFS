import subprocess
import os
import re
import time
import statistics
import random
import string

def run_command_in_background(command):
    """
    Run a command in the background using subprocess.Popen.
    """
    print(f"Starting command: {command}")
    process = subprocess.Popen(command, shell=True)
    return process

def run_command(command):
    """
    Run a command and wait for it to complete, returning elapsed time.
    """
    print(f"Executing: {command}")
    start_time = time.time()
    result = subprocess.run(command, shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    end_time = time.time()
    elapsed_time = end_time - start_time

    if result.returncode != 0:
        print(f"Error: {result.stderr.decode('utf-8')}")
    else:
        print(f"Success: {result.stdout.decode('utf-8')}")

    return elapsed_time  # Return the time taken for this command

def save_performance_results(file_name, total_time, avg_time, throughput, client_num, file_size_kb, test_type):
    """
    Save the performance results to a file inside the performance_results folder.
    """
    # Ensure the performance_results folder exists
    if not os.path.exists("performance_results"):
        os.makedirs("performance_results")

    file_path = os.path.join("performance_results", file_name)

    with open(file_path, "w") as f:
        f.write(f"Performance Metrics for {test_type} Test\n")
        f.write(f"Total Files {test_type}: {client_num}\n")
        f.write(f"File Size: {file_size_kb} KB\n")
        f.write(f"Total Time Taken: {total_time:.2f} seconds\n")
        f.write(f"Average {test_type} Time: {avg_time:.2f} seconds\n")
        f.write(f"Throughput: {throughput:.2f} KB/s\n")
    print(f"{test_type} performance results saved to {file_path}")

def generate_utf8_file(file_name, file_size_kb):
    """
    Generate a file with valid UTF-8 text content.
    """
    chars = string.ascii_letters + string.digits + " \n"
    total_chars = file_size_kb * 1024  # Total size in bytes
    with open(file_name, "w", encoding="utf-8") as f:
        for _ in range(total_chars):
            f.write(random.choice(chars))

def main():
    # Configuration
    client_num = 5        # Number of clients for testing
    file_size_kb = 2      # Size of each test file in KB
    sleep_interval = 1    # Time to wait between operations

    # Step 1: Start Master nodes in the background
    print("Starting Master nodes...")
    master_commands = [
        "target/release/master -a 127.0.0.1:50001",
        "target/release/master -a 127.0.0.1:50002",
        "target/release/master -a 127.0.0.1:50003"
    ]
    master_processes = [run_command_in_background(command) for command in master_commands]

    # Step 2: Start ChunkServer nodes in the background
    print("Starting ChunkServer nodes...")
    chunkserver_commands = [
        "target/release/chunkserver -a 127.0.0.1:50010",
        "target/release/chunkserver -a 127.0.0.1:50011"
    ]

    # Cleanup directories for ChunkServer nodes
    pattern = r"(\d+\.\d+\.\d+\.\d+:\d+)"
    for cmd in chunkserver_commands:
        match = re.search(pattern, cmd)
        if match:
            directory = match.group(1).replace(":", "_")
            os.system(f"rm -rf {directory}")

    chunkserver_processes = [run_command_in_background(command) for command in chunkserver_commands]
    time.sleep(2)  # Give servers time to start

    # Step 3: Upload Performance Test
    print("Starting upload performance test...")
    upload_times = []
    for i in range(client_num):
        file_name = f"test_file_upload_{i}.txt"
        generate_utf8_file(file_name, file_size_kb)

        upload_command = f"target/release/client upload {file_name}"
        elapsed_time = run_command(upload_command)
        upload_times.append(elapsed_time)

        os.remove(file_name)
        time.sleep(sleep_interval)

    # Calculate and save upload metrics
    # total_time = sum(upload_times)
    # avg_time = statistics.mean(upload_times)
    # throughput = (client_num * file_size_kb) / total_time
    # save_performance_results("upload_performance_results.txt", total_time, avg_time, throughput, client_num, file_size_kb, "Upload")

    # Step 4: Read Performance Test
    print("Starting read performance test...")
    read_times = []
    for i in range(client_num):
        file_name = f"test_file_read_{i}.txt"
        generate_utf8_file(file_name, file_size_kb)

        upload_command = f"target/release/client upload {file_name}"
        run_command(upload_command)

        read_command = f"target/release/client read {file_name}"
        elapsed_time = run_command(read_command)
        read_times.append(elapsed_time)

        os.remove(file_name)
        time.sleep(sleep_interval)

    # Calculate and save read metrics
    total_time_read = sum(read_times)
    avg_time_read = statistics.mean(read_times)
    throughput_read = (client_num * file_size_kb) / total_time_read
    save_performance_results("read_performance_results.txt", total_time_read, avg_time_read, throughput_read, client_num, file_size_kb, "Read")

    # Wait for all background processes to finish
    for process in master_processes + chunkserver_processes:
        process.terminate()

if __name__ == "__main__":
    main()
