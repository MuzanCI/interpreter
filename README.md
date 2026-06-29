# MuzanCI Interpreter

An interpreter for the Python-like pipeline configuration language used by MuzanCI.

The muzan.py file defines a set of rules, which look like Python function calls.

There are two types of rules:
- job rules
- pipeline rules

## Job rules

A job rule defines a job, which is a unit of work that executes in an ephemeral, isolated shell environment.

A job may have multiple steps, and each step is executed in the same shell environment as the previous step. This allows for filesystem changes to be preserved between steps.

If any step fails (exit code != 0), then the job stops immediately and is marked as failed.

## Pipeline rules

A pipeline rule defines a pipeline, which is a 


```py
pipeline(
    name = "example_pipeline",
    description = "An example pipeline demonstrating the MuzanCI interpreter.",
    deps = ["job_a"],
)

job(
    id = "job_a",
    image = "freebsd:15.1-RELEASE",
    steps = [

    ]
)



job(
    id = "job_b",
    depends_on = ["job_a"],
)

# You can even use loops natively!
for i in range(3):
    job(
        id = "dynamic_subtask_{}".format(i),
        depends_on = ["job_b"]
    )
```