struct Params {
    chunk_samples: u32,
    // Must be >= 1.
    trace_samples: u32,
    pixel_count: u32,
    w: u32,
    h: u32,
    scale_y: f32,
    offset: f32
};

@group(0) @binding(0)
var<storage, read> input: array<f32>;

@group(0) @binding(1)
var<storage, read_write> output: array<u32>;

@group(0) @binding(2)
var<uniform> params: Params;

@compute @workgroup_size(64)
fn render(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let index = global_id.x;

    if (index >= params.pixel_count) {
        return;
    }

    let pix_y = index % params.h;
    let pix_x = index / params.h;
    var density: u32 = 0;

    // Calculate the trace range for the current pixel column.
    let i_start = min(params.trace_samples - 1, u32(f32(params.chunk_samples) * (f32(pix_x) / f32(params.w))));
    let i_end = min(params.trace_samples - 1, u32(f32(params.chunk_samples) * (f32(pix_x + 1) / f32(params.w))));
    
    let mid = f32(params.h / 2);
    let y = mid - f32(pix_y);

    // Following is the rendering algorithm with easy-to-read implementation.
    // For each trace segment, we test if the ordinate of the current pixel is included in the
    // ordinate range of the segment.
    //
    // for (var i = i_start; i < i_end; i++) {
    //     let p0 = input[i];
    //     let p1 = input[i + 1];
    //     if (((y >= p0) && (y <= p1)) || ((y >= p1) && (y <= p0))) {
    //         density += 1;
    //     }
    // }
    //
    // Some calculation can be kept for the next loop, leading to the following faster
    // implementation below.
    
    var p0 = (input[i_start] + params.offset) * params.scale_y;
    var ca0 = (y <= p0);
    var cb0 = (y >= p0);
    for (var i = i_start; i < i_end; i+=1) {
        let p1 = (input[i + 1] + params.offset) * params.scale_y;
        let ca1 = (y <= p1);
        let cb1 = (y >= p1);
        // Cast from bool to u32 is slightly faster than doing conditional incrementation.
        density += u32((cb0 && ca1) || (cb1 && ca0));
        p0 = p1;
        ca0 = ca1;
        cb0 = cb1;
    }

    // Following is another possible implementation trying to better fit classical GPU operations.
    // Unfortunately it does not appear to be any faster than the other implementations.
    //
    // for (var i = i_start; i < i_end; i++) {
    //     var p0 = input[i];
    //     let p1 = input[i + 1];
    //     let k = vec4(y, p0, y, p1);
    //     let l = vec4(p1, y, p0, y);
    //     let m = step(k, l);
    //     let inside = m.x * m.y + m.z * m.w;
    //     density += u32(inside);
    // }

    output[index] = density;
}
