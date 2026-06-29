external_job = Job(
    name="external_job",
    steps=[Step(name="external_step", command="echo 'Hello from external job!'")],
)
