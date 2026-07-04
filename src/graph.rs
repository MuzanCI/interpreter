use std::collections::{HashMap, HashSet};

use crate::{JobGraph, JobId, Pipeline};

/// A sequence of job IDs that form a cycle. The first and last job IDs are the same, indicating the start and end of the cycle.
pub type Cycle = Vec<JobId>;

/// Walk the job graph starting from each pipeline target, and collect all reachable jobs and edges.
/// If a cycle is detected, return the cycle as a list of job IDs.
pub fn walk_targets(
    pipeline: Pipeline,
    job_deps: JobGraph,
) -> Result<(HashSet<JobId>, JobGraph), Cycle> {
    let mut visited = HashSet::new();
    let mut path_set = HashSet::new();
    let mut path_vec = Vec::new();

    let mut reachable_jobs = HashSet::new();
    let mut reachable_job_deps: JobGraph = HashMap::new();

    pipeline.targets.into_iter().try_for_each(|target| {
        dfs(
            target,
            &job_deps,
            &mut visited,
            &mut path_set,
            &mut path_vec,
            &mut reachable_jobs,
            &mut reachable_job_deps,
        )?;

        Ok::<(), Cycle>(())
    })?;

    Ok((reachable_jobs, reachable_job_deps))
}

/// Helper recursive DFS function to traverse the graph and detect cycles.
fn dfs(
    node: JobId,
    graph: &JobGraph,
    visited: &mut HashSet<JobId>,
    path_set: &mut HashSet<JobId>,
    path_vec: &mut Vec<JobId>,
    reachable_jobs: &mut HashSet<JobId>,
    reachable_job_deps: &mut JobGraph,
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
    if let Some(deps) = graph.get(&node) {
        deps.into_iter().try_for_each(|dep| {
            // Recurse into the dependency
            dfs(
                dep.clone(),
                graph,
                visited,
                path_set,
                path_vec,
                reachable_jobs,
                reachable_job_deps,
            )?;

            // Add the edge to the reachable job dependencies
            reachable_job_deps
                .entry(node.clone())
                .or_default()
                .insert(dep.clone());

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
