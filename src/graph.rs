use std::collections::{HashMap, HashSet};

use crate::{JobId, Need, Pipeline, collect::JobRegistry};

/// A mapping from a JobId to the list of JobIds that the job depends on.
/// Also known as a list adjacency list representation of a directed graph.
pub type JobGraph = HashMap<JobId, HashSet<Need>>;

/// A sequence of job IDs that form a cycle. The first and last job IDs are the same, indicating the start and end of the cycle.
pub type Cycle = Vec<JobId>;

/// Walk the job graph starting from each pipeline target, and collect all reachable jobs and edges.
/// If a cycle is detected, return the cycle as a list of job IDs.
pub fn walk_targets(
    pipeline: Pipeline,
    job_registry: JobRegistry,
) -> Result<HashSet<JobId>, Cycle> {
    let mut visited = HashSet::new();
    let mut path_set = HashSet::new();
    let mut path_vec = Vec::new();

    let mut reachable_jobs = HashSet::new();

    pipeline.targets.into_iter().try_for_each(|need| {
        dfs(
            need.job_id,
            &job_registry,
            &mut visited,
            &mut path_set,
            &mut path_vec,
            &mut reachable_jobs,
        )?;

        Ok::<(), Cycle>(())
    })?;

    Ok(reachable_jobs)
}

/// Helper recursive DFS function to traverse the graph and detect cycles.
fn dfs(
    node: JobId,
    job_registry: &JobRegistry,
    visited: &mut HashSet<JobId>,
    path_set: &mut HashSet<JobId>,
    path_vec: &mut Vec<JobId>,
    reachable_jobs: &mut HashSet<JobId>,
) -> Result<(), Cycle> {
    // Add to reachable jobs immediately upon discovery
    reachable_jobs.insert(node.clone());

    // If the node is currently in the recursion stack, we've hit a back-edge (a cycle).
    if path_set.contains(&node) {
        // Extract the cycle path from the stack
        let start_idx = path_vec
            .iter()
            .position(|id| id == &node)
            .expect("Node must exist in path_vec if it is in path_set");

        let mut cycle = path_vec[start_idx..].to_vec();
        cycle.push(node); // Append the start node to the end to close the loop

        return Err(cycle);
    }

    // If we've already fully explored this node's subtree in a previous path,
    // we don't need to do it again (Dynamic Programming / Memoization).
    if visited.contains(&node) {
        return Ok(());
    }

    // Push current node onto the visitation stack
    path_set.insert(node.clone());
    path_vec.push(node.clone());

    // Traverse dependencies
    if let Some(needs) = job_registry.get(&node).map(|job| &job.needs) {
        needs.into_iter().try_for_each(|need| {
            // Recurse into the dependency
            dfs(
                need.job_id.clone(),
                job_registry,
                visited,
                path_set,
                path_vec,
                reachable_jobs,
            )?;

            Ok::<(), Cycle>(())
        })?;
    }

    // Pop the node off the visitation stack as we backtrack
    path_vec.pop();
    path_set.remove(&node);

    // Mark as permanently visited so we never traverse its subtree again
    visited.insert(node);

    Ok(())
}
