var wav = require('wav')
var fs = require('fs')
var Chunker = require('stream-chunker')
var { Transform } = require('stream')
var bs = require('binary-search')

var inputFile = process.argv[2]
var outputFile = process.argv[3]
var levelsFile = process.argv[2] + '.levels'

var levels = JSON.parse(fs.readFileSync(levelsFile, 'utf8'))

var reader = fs.createReadStream(inputFile).pipe(wav.Reader())

reader.on('format', function (format) {
  // the WAVE header is stripped from the output of the reader
  var channelBytes = format.blockAlign / format.channels
  var writer = wav.FileWriter(outputFile, format)
  var processed = 0
  var transform = new Transform({
    transform (chunk, encoding, cb) {
      for (var i = 0; i < chunk.length; i += format.blockAlign) {
        for (var channel = 0; channel < format.channels; channel += 1) {
          var time = (processed + i) / format.byteRate
          var chanOffset = channelBytes * channel
          var value = readFormat(chunk, i + chanOffset, format)
          writeFormat(chunk, value * getVolume(time, levels), i + chanOffset, format)
        }
      }
      this.push(chunk)
      processed += chunk.length
      cb()
    }
  })
  reader.pipe(Chunker(format.blockAlign * 1024)).pipe(transform).pipe(writer)
})

function writeFormat (buf, value, offset, format) {
  if (format.bitDepth === 16 && format.endianness === 'LE') {
    return buf.writeInt16LE(clip(value, -0x8000, 0x7FFF), offset)
  } else {
    throw new Error('Unsupported format')
  }
}

function readFormat (buf, offset, format) {
  if (format.bitDepth === 16 && format.endianness === 'LE') {
    return buf.readInt16LE(offset)
  } else {
    throw new Error('Unsupported format')
  }
}

function getVolume (time, events) {
  var index = bs(events, time, function (a, b) {
    return a.time - b
  })

  if (index >= 0) {
    return events[index].value
  } else {
    var before = events[(index * -1) - 2]
    var after = events[(index * -1) - 1]

    if (before && after) {
      var range = after.value - before.value
      var pos = (time - before.time) / (after.time - before.time)
      return before.value + (range * pos)
    } else if (before) {
      return before.value
    } else if (after) {
      return after.value
    } else {
      return 1
    }
  }
}

function clip (value, min, max) {
  if (value > max) return max
  if (value < min) return min
  return value
}
