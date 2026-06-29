external_job = job(
    name="external_job",
    steps=[step(name="external_step", command="echo 'Hello from external job!'")],
)
