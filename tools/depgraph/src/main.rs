// Copyright (c) 2023 Marceline Cramer
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// This file is part of Hearth.
//
// Hearth is free software: you can redistribute it and/or modify it under the
// terms of the GNU Affero General Public License as published by the Free
// Software Foundation, either version 3 of the License, or (at your option)
// any later version.
//
// Hearth is distributed in the hope that it will be useful, but WITHOUT ANY
// WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License for more
// details.
//
// You should have received a copy of the GNU Affero General Public License
// along with Hearth. If not, see <https://www.gnu.org/licenses/>.

use std::collections::HashMap;

use anyhow::Context;
use cargo_metadata::{MetadataCommand, PackageId};
use petgraph::{algo::all_simple_paths, visit::EdgeRef, Graph};

fn main() -> anyhow::Result<()> {
    // fetch workspace metadata
    let metadata = MetadataCommand::new()
        .exec()
        .context("Retrieving workspace metadata")?;

    // get length of path to trim
    // add one to trim off trailing slash
    let root_len = metadata.workspace_root.into_string().chars().count() + 1;

    // retrieve resolved depgraph
    let resolve = metadata
        .resolve
        .context("Couldn't obtain dependency graph. Your cargo version may be too old.")?;

    // helper to fetch package metadata by ID
    let find_package = |id: &str| {
        metadata
            .packages
            .iter()
            .find(|pkg| pkg.id.repr == id)
            .with_context(|| format!("Couldn't find package {}", id))
    };

    // map workspace member IDs to subdirectory
    let mut members = HashMap::new();

    // reverse map of subdirectory to package IDs
    let mut subdirectories = HashMap::<String, Vec<PackageId>>::new();

    // add workspace members
    for id in metadata.workspace_members {
        let package = find_package(&id.repr)?;
        let name = package.name.clone().replace('-', "_");

        let absolute_path = package.manifest_path.clone().into_string();
        let relative_path = &absolute_path[root_len..];

        let subdirectory = relative_path
            .split('/')
            .next()
            .with_context(|| format!("Path {:?} not in subdirectory", relative_path))?
            .to_string();

        // insert package into subdirectory
        subdirectories
            .entry(subdirectory)
            .or_default()
            .push(id.clone());

        // insert member by ID
        members.insert(id, name);
    }

    // begin .dot file
    println!("digraph {{");
    println!("    node [shape=box];");
    println!("    edge [weight=500;arrowhead=none;];");

    // set graph attributes
    println!("    graph [");
    println!("        rankdir=LR;");
    println!("        overlap=false;");
    println!("        splines=compound;");
    println!("        ranksep=1.0;");
    println!("        nodesep=0.15;");
    println!("    ]");

    // add each subdirectory subgraph
    for (subdir, ids) in subdirectories {
        // generate cluster name
        let cluster = format!("cluster_{subdir}");

        // begin subgraph
        println!("    subgraph {cluster} {{");
        println!("        label = <<b>{subdir}/</b>>;");
        println!("        color = lightgrey;");
        println!("        font = Arial;");
        println!("        fontsize = 18;");

        // add each member
        for id in ids.iter() {
            let package = find_package(&id.repr)?;
            println!("        {};", package.name.clone().replace('-', "_"));
        }

        // end subgraph
        println!("    }}");
    }

    // initialize graph
    let mut graph = Graph::new();

    // maps member IDs to node
    let mut nodes = HashMap::new();

    // add all member nodes to graph
    for (member, name) in members.clone() {
        let idx = graph.add_node(name);
        nodes.insert(member, idx);
    }

    // add dependencies
    for node in resolve.nodes {
        // get dependent node
        let Some(dependent) = nodes.get(&node.id) else {
            continue;
        };

        // add dependencies
        for id in node.dependencies {
            // get dependency node
            let Some(dependency) = nodes.get(&id) else {
                continue;
            };

            // add edge
            graph.add_edge(*dependent, *dependency, ());
        }
    }

    // dedup transitive deps
    // adapted from https://github.com/jplatte/cargo-depgraph
    for idx in graph.node_indices().collect::<Vec<_>>() {
        // walk down the dependency graph from this node
        let mut outgoing = graph
            .neighbors_directed(idx, petgraph::Direction::Outgoing)
            .detach();

        // get all indirect dependents
        while let Some((edge_idx, node_idx)) = outgoing.next(&graph) {
            // try to find an indirect dependency
            let any_paths = all_simple_paths::<Vec<_>, _>(&graph, idx, node_idx, 1, None)
                .next()
                .is_some();

            // if there's an indirect dep, remove the direct dep
            if any_paths {
                graph.remove_edge(edge_idx);
            }
        }
    }

    // add graph edges to .dot file
    for edge in graph.edge_references() {
        // get labels of each end of edge
        let dependency = graph.node_weight(edge.source()).unwrap();
        let dependent = graph.node_weight(edge.target()).unwrap();

        // add edge to .dot
        // :e and :w set the ports to connect the edge to
        println!("    {dependent}:e -> {dependency}:w;");
    }

    // end .dot file
    println!("}}");

    Ok(())
}
