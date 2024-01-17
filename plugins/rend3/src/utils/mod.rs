// Copyright (c) 2023 the Hearth contributors.
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

use std::marker::PhantomData;

use bytemuck::Pod;
use wgpu::{util::DeviceExt, *};

/// A growable array of GPU memory.
pub struct GpuVector<T> {
    buffer: Buffer,
    length: u64,
    capacity: u64,
    label: Option<String>,
    usage: BufferUsages,
    _data: PhantomData<T>,
}

impl<T: Pod> GpuVector<T> {
    /// Creates a new GPU vector.
    pub fn new(device: &Device, label: Option<String>, usage: BufferUsages) -> Self {
        let usage = usage | BufferUsages::COPY_DST;

        Self {
            buffer: device.create_buffer(&BufferDescriptor {
                label: label.as_deref(),
                size: 0,
                usage,
                mapped_at_creation: false,
            }),
            length: 0,
            capacity: 0,
            label,
            usage,
            _data: PhantomData,
        }
    }

    /// Updates the GPU-side contents of this vector, increasing capacity if needed.
    pub fn update(&mut self, device: &Device, queue: &Queue, data: &[T]) {
        if self.capacity >= data.len() as u64 {
            queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(data));
        } else {
            self.capacity = data.len().next_power_of_two() as u64;
            let mut contents: Vec<u8> = bytemuck::cast_slice(data).to_vec();
            let size = self.capacity as usize * std::mem::size_of::<T>();
            contents.resize(size, 0);

            self.buffer = device.create_buffer_init(&util::BufferInitDescriptor {
                label: self.label.as_deref(),
                contents: contents.as_slice(),
                usage: self.usage,
            });
        }

        self.length = data.len() as u64;
    }

    /// Tests if this vector is empty.
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// Retrieves the length in elements of this vector.
    pub fn len(&self) -> u64 {
        self.length
    }

    /// Gets a handle to the underlying GPU buffer.
    pub fn get_buffer(&self) -> &Buffer {
        &self.buffer
    }
}

/// Dynamically-allocated GPU mesh data.
pub struct DynamicMesh<T> {
    vertices: GpuVector<T>,
    indices: GpuVector<u32>,
}

impl<T: Pod> DynamicMesh<T> {
    /// Creates a new dynamic mesh.
    pub fn new(device: &Device, label: Option<String>) -> Self {
        Self {
            vertices: GpuVector::new(device, label.clone(), BufferUsages::VERTEX),
            indices: GpuVector::new(device, label, BufferUsages::INDEX),
        }
    }

    /// Update the mesh with the given vertices and indices, increasing
    /// capacity if needed.
    pub fn update(&mut self, device: &Device, queue: &Queue, vertices: &[T], indices: &[u32]) {
        self.vertices.update(device, queue, vertices);
        self.indices.update(device, queue, indices);
    }

    /// Bind this mesh to the given render pass and perform a draw operation.
    pub fn draw<'a>(&'a self, rpass: &mut RenderPass<'a>) {
        let vs = self.vertices.get_buffer();
        let is = self.indices.get_buffer();
        let index_num = self.indices.len().try_into().unwrap();

        rpass.set_vertex_buffer(0, vs.slice(..));
        rpass.set_index_buffer(is.slice(..), IndexFormat::Uint32);
        rpass.draw_indexed(0..index_num, 0, 0..1);
    }
}
