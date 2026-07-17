# MuzanCI Interpreter

An interpreter for the Python-like pipeline configuration language used by MuzanCI.

## Config Example

```py
build_step = Job(
    name="build_job",
    steps=[
        Step(
            name="build",
            command="cargo build --release",
        ),
    ],
)

test_step = Job(
    name="test_job",
    needs=[build_step],
    steps=[
        Step(
            name="test",
            command="cargo test --release",
        ),
    ],
)

my_pipeline = Pipeline(
    name="my_pipeline",
    when=[
        Push(include_branches=["main"]),
    ],
    targets=[test_step],
)
```

## CLI Usage

```sh
$ interpreter
```

Prints help information about the program.

---

```sh
$ interpreter show
    --input FILE
    --format {ascii,json,dotgraph}
```

Parses the pipeline config and prints the dependency graph in the specified format.

```
---------------------------
| Pipeline: "my_pipeline" |
---------------------------
            |
            ▼
   -------------------
   | Job: "test_job" |
   -------------------
            |
            ▼
   --------------------
   | Job: "build_job" |
   --------------------
```

---

```sh
$ interpreter check
    --input FILE
```

Parses the pipeline config and checks for syntax errors and cyclical dependencies.

```
Parsed 1 file.
No syntax errors.
No cycles detected.
```

---

```sh
$ interpreter git-clone
    --url URL
    --branch BRANCH
    --commit COMMIT_SHA
    --target-dir PATH
    --input FILE
    --format {ascii,json,dotgraph}
```

Clones a Git repository at the specified URL, branch, and commit SHA into the target directory.
Then, parses the pipeline config and prints the dependency graph in the specified format.

This command is intended for CI runners.

## Concepts

- Step: a single shell command that executes within a Job sandbox.
- Secret: an environment variable that is injected into a Step during runtime.
- Job: a sequence of Steps that execute within a sandbox.
- Pipeline: a set of target Jobs and a set of events for when the Jobs should be executed.

### Steps

A Step is a single shell command that executes within a Job sandbox.
A Step may have multiple Secrets, which will be injected into the Step's environment during runtime.
Steps within the same Job share the same filesystem, but Secrets are cleared after each Step.
If a Step fails (exit code != 0), then the Step is considered failed. The Step's Job stops immediately and is also marked as failed.

```py
build_step = Step(
    name="build",
    command="cargo build --release",
)
```

### Secrets

A Secret is an object with a name and a key. The name is the environment variable name that will be injected into the Step during runtime, and the key is the key used by the runner to fetch the Secret's value.

```py
Secret(
    name="AWS_SECRET_ACCESS_KEY",
    key="aws_secret",
)
```

```py
upload_step = Step(
    name="upload",
    command="aws s3 cp target/release/my_binary s3://my-bucket/",
    secrets=[
        Secret(
            name="AWS_SECRET_ACCESS_KEY",
            key="aws_secret",
        )
    ]
)
```

### Jobs

A Job is a series of Steps that execute within an ephemeral and isolated sandbox.
All Steps within a Job share the same root filesystem, but Secrets are cleared after each Step.
If any Step fails (exit code != 0), then the Job stops immediately and is marked as failed.

```py
build_job = Job(
    name="build_job",
    steps=[
        Step(
            name="build",
            command="cargo build --release",
        ),
    ],
)
```

A Job may depend on the success (or failure) of other Jobs.

```py
test_job = Job(
    name="test_job",
    needs=[build_job],
    steps=[
        Step(
            name="test",
            command="cargo test --release",
        ),
    ],
)
```

By default, a Job will depend on the success of a Job in the `needs` list.
To configure a Job to run when a Job fails, use `Job.failed` in the `needs` list.

```py
clean_up_job = Job(
    name="clean_up_job",
    needs=[build_job.failed],
    steps=[
        Step(
            name="clean_up",
            command="rm -rf target/",
        ),
    ],
)
```


### Pipelines

A Pipeline defines a set of Jobs and a set of events for when the Jobs (and their dependencies) should be executed.

```py
# A pipeline that targets the build_job and runs on every push to branch `main`.
pipeline_on_push_main = Pipeline(
    name="full_pipeline",
    when=[
        Push(include_branches=["main"]),
    ],
    needs=["build_job"],
)

# A pipeline that runs on whenever a pull request is opened, re-opened, or the HEAD commit is updated.
pipeline_on_pr = Pipeline(
    name="pr_pipeline",
    when=[
        PullRequest(),
    ],
    targets=["build_job"],
)
```
