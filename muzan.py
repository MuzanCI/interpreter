# muzan.star — MuzanCI pipeline configuration example.
#
# The primitives secret(), step(), job(), and pipeline() are provided by the
# interpreter as Rust StarlarkValue globals.  This file shows how to compose
# them into a real CI workflow.

# ------------------------------------------------------------------------------
# Reusable step helpers (Starlark-level wrappers around the step() primitive).
# ------------------------------------------------------------------------------


def checkout(repo, branch):
    return Step(
        name="checkout {} {}".format(repo, branch),
        command="git checkout {} {}".format(repo, branch),
    )


def upload(source, destination, secrets=[]):
    return Step(
        name="upload {} {}".format(source, destination),
        command="aws s3 upload {} {}".format(source, destination),
        secrets=secrets,
    )


def download(source, destination, secrets=[]):
    return Step(
        name="download {} {}".format(source, destination),
        command="aws s3 download {} {}".format(source, destination),
        secrets=secrets,
    )


def attest(glob, oidc_issuer, secrets=[]):
    command = "attest {} {}".format(glob, oidc_issuer)
    return Step(
        name=command,
        command=command,
        secrets=secrets,
    )


# ------------------------------------------------------------------------------
# Pipeline definition
# ------------------------------------------------------------------------------

aws_secret = Secret(
    name="aws_secret",
    key="AWS_SECRET_ACCESS_KEY",
)

build_job = Job(
    name="build_job",
    steps=[
        checkout(
            repo=GIT_REPO,  # injected by the interpreter
            branch=GIT_BRANCH,
        ),
        Step(
            name="build",
            command="cargo build --release",
        ),
        attest(
            glob="target/release/my_binary",
            oidc_issuer="https://oidc.example.com",
        ),
        upload(
            source="target/release/my_binary",
            destination="s3://my-bucket/{}/my_binary".format(GIT_COMMIT),
            secrets=[aws_secret],
        ),
    ],
)

test_job = Job(
    name="test_job",
    depends_on=[build_job],
    steps=[
        download(
            source="s3://my-bucket/{}/my_binary".format(GIT_COMMIT),
            destination="./my_binary",
        ),
        Step(
            name="test",
            command="./my_binary --test --output results.json",
        ),
        attest(
            glob="results.json",
            oidc_issuer="https://oidc.example.com",
        ),
    ],
)

load("external.py", "external_job")

# pipeline() is called for its side effect: it registers the pipeline in the
# interpreter's collector.  It does not need to be assigned to a variable.
for arch in ["x86_64", "arm64"]:
    Pipeline(
        name=f"release_{arch}",
        predicates=[lambda: GIT_BRANCH == "main"],
        depends_on=[test_job, external_job],
    )

    # pipeline(
    #     name=f"another_{arch}",
    #     when=[
    #         push(),
    #     ],
    #     depends_on=[test_job],
    # )
