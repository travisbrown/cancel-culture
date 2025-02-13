idiot proof


Step 1: Clone the Repository
Open your VSCode Terminal (or your system’s terminal).

Clone the repository using the following command:

bash
Copy
git clone https://github.com/travisbrown/cancel-culture.git
Navigate to the project directory:

bash
Copy
cd cancel-culture

Step 2: Switch to the "topic/no-api" Branch
Checkout the topic/no-api branch by running:

bash
Copy
git checkout topic/no-api
Step 3: Compile the Project
To compile the project, you'll need Rust and Cargo installed on your system.

If you don’t have Rust and Cargo, you can install them from here.
Once installed, run the following command to compile the project in release mode:

bash
Copy
cargo build --release

Step 4: Create a Local Store Directory (Optional)
If you want to store downloaded snapshots locally (to avoid redownloading), create a directory called store:

bash
Copy
mkdir store

Step 5: Run the Program
Now, you can run the program with the command below. Replace SCREEN_NAME with the actual username of the account you're interested in.

bash
Copy
target/release/twcc -vvv deleted-tweets --include-failed --no-api --report --store store/ SCREEN_NAME > SCREEN_NAME.md

This command will generate a report about deleted tweets for the given SCREEN_NAME and save it as a .md file.

Step 6: Publish the .md File

Step 1: Initialize a Git Repository (if not already done)
If your project is not already initialized as a Git repository, follow these steps:

Open VSCode and open your project folder.

Open the Terminal in VSCode (Ctrl + ~ or Cmd + ~ on macOS).

If your project folder isn’t a Git repository, initialize it by running:

bash
Copy
git init

Step 2: Stage Your Files
Now that your project is initialized as a Git repository, you need to add files to Git.

Stage all files in the repository (this adds them to the staging area to be committed):

bash
Copy
git add .
This will stage all files in the folder. If you only want to add specific files, replace . with the filenames.

Step 3: Commit Your Changes
After staging the files, commit them to the repository with a meaningful message:

Run the following command:

bash
Copy
git commit -m "Your commit message"
Replace "Your commit message" with a description of the changes you made.

Use 'git push' tp publish file

esteck@CNNNY-MBPRO654 cancel-culture % git push
