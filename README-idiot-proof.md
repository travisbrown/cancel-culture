## Overview

Here is how to use this code.

## Step 1. Clone the Repository

Open your VSCode Terminal (or your system’s terminal).

Clone the repository using the following command:

```
git clone https://github.com/travisbrown/cancel-culture.git
```
Navigate to the project directory:

```
cd cancel-culture
```
## Step 2: Switch to the "topic/no-api" Branch

Checkout the topic/no-api branch by running:

```
git checkout topic/no-api
```

## Step 3: Compile the Project

To compile the project, you'll need Rust and Cargo installed on your system.

If you don’t have Rust and Cargo, you can install them from here.

Once installed, run the following command to compile the project in release mode:

```
cargo build --release
```

## Step 4: Create a Local Store Directory (Optional per chatgpt)
If you want to store downloaded snapshots locally (to avoid redownloading), create a directory called store:

```
mkdir store
```

## Step 5: Run the Program

Now, you can run the program with the command below. Replace SCREEN_NAME with the actual username of the account you're interested in.

```
target/release/twcc -vvv deleted-tweets --include-failed --no-api --report --store store/ SCREEN_NAME > SCREEN_NAME.md
```

This command will generate a report about deleted tweets for the given SCREEN_NAME and save it as a .md file.

## Step 6: Copy and paste the .md File to Gists 

[Gist](https://gist.github.com/)

