use std::collections::HashMap;
use std::collections::HashSet;

pub type Graph<T> = HashMap<T, HashSet<T>>;

/// A sequence of job IDs that form a cycle. The first and last job IDs are the same, indicating the start and end of the cycle.
pub type Cycle<T> = Vec<T>;

/// Walk the graph starting from each pipeline target, and collect all reachable jobs.
/// If a cycle is detected, return the cycle.
pub fn reachable<T>(source_nodes: &HashSet<T>, graph: &Graph<T>) -> Result<HashSet<T>, Cycle<T>>
where
    T: Clone + Eq + std::hash::Hash,
{
    let mut reachable = HashSet::new();

    // Forwards pass to capture reachable jobs from the pipeline's needs
    let mut visited = HashSet::new();
    let mut path_set = HashSet::new();
    let mut path_vec = Vec::new();
    source_nodes
        .iter()
        .try_for_each(|node| -> Result<(), Cycle<T>> {
            dfs(
                node.clone(),
                &graph,
                &mut visited,
                &mut path_set,
                &mut path_vec,
                &mut reachable,
            )?;

            Ok(())
        })?;

    Ok(reachable)
}

/// Helper recursive DFS function to traverse the graph and detect cycles.
fn dfs<T>(
    node: T,
    graph: &Graph<T>,
    visited: &mut HashSet<T>,
    path_set: &mut HashSet<T>,
    path_vec: &mut Vec<T>,
    reachable: &mut HashSet<T>,
) -> Result<(), Cycle<T>>
where
    T: Clone + Eq + std::hash::Hash,
{
    // Add to reachable jobs immediately upon discovery
    reachable.insert(node.clone());

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
    graph
        .get(&node)
        .unwrap_or(&HashSet::new())
        .iter()
        .try_for_each(|neighbor| {
            dfs(
                neighbor.clone(),
                graph,
                visited,
                path_set,
                path_vec,
                reachable,
            )
        })?;

    // Pop the node off the visitation stack as we backtrack
    path_vec.pop();
    path_set.remove(&node);

    // Mark as permanently visited so we never traverse its subtree again
    visited.insert(node);

    Ok(())
}
