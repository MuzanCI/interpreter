build_job = Job(
    name="build_job",
    command="cargo build",
    needs=[],
)

test_job = Job(
    name="test_job",
    command="cargo test",
    needs=[build_job.completed],
)

clean_job = Job(
    name="clean_job",
    command="cargo clean",
    needs=[test_job.failed],
)

unused_job = Job(
    name="unused_job",
    command="echo 'This job is not used in the pipeline'",
    needs=[],
)

pipeline = Pipeline(
    name="my_pipeline",
    targets=[
        test_job,
        # unused_job,
    ],
)
