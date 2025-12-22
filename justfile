compile_shaders:
    slangc src/textslabs.slang -target spirv -entry vertexMain -stage vertex -o slangc_output/text.vert.spv
    slangc src/textslabs.slang -target spirv -entry fragmentMain -stage fragment -o slangc_output/text.frag.spv
