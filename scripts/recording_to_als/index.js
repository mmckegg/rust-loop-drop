#!/usr/bin/env node

var execFileSync = require('child_process').execFileSync
var Path = require('path')
var fs = require('fs')
var ejs = require('ejs')
var template = ejs.compile(fs.readFileSync(Path.join(__dirname, 'template.als.ejs'), 'utf8'))

// strip off .events (if passed in) and use raw file instead
var inPath = process.argv[2].replace(/\.events$/, '')
var eventsPath = `${inPath}.events`

var extName = Path.extname(inPath)
var baseName = Path.basename(inPath, extName)

// output paths
var projectPath = process.argv[3]
var projectName = Path.basename(projectPath)
var alsPath = Path.join(projectPath, `${projectName}.als`)
var tempo = null
var ticks = []
var duration = 0
var length = 0

// sync the K-Mix clock and Raspberry Pi 
var recordingTimeMultiplier = 0.9999541021
var tempoMultiplier = 1 / recordingTimeMultiplier

fs.mkdirSync(projectPath)

var tracks = [
  {
    id: 8,
    isTempoMaster: true,
    name: 'drums',
    channels: [1],
    fileName: `${baseName}-sampler.wav`,
    volumeEvents: []
  }, {
    id: 9,
    name: 'ext',
    channels: [5],
    fileName: `${baseName}-ext.wav`,
    volumeEvents: []
  }, {
    id: 10,
    name: 'bass',
    channels: [2],
    fileName: `${baseName}-bass.wav`,
    volumeEvents: []
  }, {
    id: 11,
    name: 'synth',
    channels: [3,4],
    fileName: `${baseName}-synth.wav`,
    volumeEvents: []
  }, {
    id: 12,
    name: 'fx',
    channels: [7, 8],
    fileName: `${baseName}-fx.wav`,
    volumeEvents: []
  }
]

var tempoEvents = []
var timeAtLastTempoChange = 0
var beatAtLastTempoChange = 0
var tickDurations = []

console.log('Parsing events...')
fs.readFileSync(eventsPath, 'utf8').split('\n').forEach(line => {
  let event = tryParse(line)
  if (event) {
    var time = event[0] * recordingTimeMultiplier
    var type = event[1]

    var lastTempoEvent = tempoEvents[tempoEvents.length - 1]
    var lastTempo = lastTempoEvent ? lastTempoEvent.tempo : 120
    var timeSinceTempoChange = time - timeAtLastTempoChange
    var beatDuration = 60 / lastTempo
    var beat = beatAtLastTempoChange + (timeSinceTempoChange / beatDuration)

    if (type === 'tick') {
      duration = time
      length = beat
      if (ticks.length) {
        tickDurations.push(time - ticks[ticks.length - 1])
      }
      ticks.push(time)
    } else if (type === 'channel_volume') {
      var track = tracks[event[2]]
      if (track) {
        var value = event[3] / 127
        var lastEvent = track.volumeEvents[track.volumeEvents.length - 1]
        if (lastEvent && time - lastEvent.time > 0.1) {
          // don't interpolate if events are more than 0.1 s apart
          track.volumeEvents.push({time: time - 0.05, beat: beat - (0.05 / beatDuration), value: lastEvent.value})
        }
        track.volumeEvents.push({time, beat, value: value * value * 1.618})
      }
    } else if (type === 'tempo') {
      if (!tempo) tempo = event[2]
      timeAtLastTempoChange = time
      beatAtLastTempoChange = beat
      tempoEvents.push({
        time,
        beat,
        tempo: event[2] * tempoMultiplier
      })
    }
  }
})

var lastTempo = tempoEvents[tempoEvents.length - 1] ? tempoEvents[tempoEvents.length - 1].tempo : tempo
tempoEvents.push({
  time: duration, beat: length, tempo: lastTempo * tempoMultiplier
})

console.log('Exporting tracks...')
tracks.forEach(track => {
  var outPath = Path.join(projectPath, track.fileName)
  execFileSync('sox', [
    inPath, outPath, 'remix'
  ].concat(track.channels))
  track.duration = length

  // if this track has a lot of volume events, apply them directly
  // to the file instead of including in als
  if (track.volumeEvents.length > 500) {
    var ext = Path.extname(track.fileName)
    var leveledName = Path.basename(track.fileName, ext) + '-with-levels' + ext
    var leveledPath = Path.join(projectPath, leveledName)
    fs.writeFileSync(outPath + '.levels', JSON.stringify(track.volumeEvents))
    execFileSync('node', [
      Path.join(__dirname, './apply-audio-levels'), outPath, leveledPath
    ])
    track.fileName = leveledName
    track.volumeEvents = []
  }
})

addProjectInfo(projectPath)

var output = template({
  tracks, tempo, tempoEvents
})

fs.writeFileSync(alsPath, output)

console.log(`Exported to ${alsPath}`)

function addProjectInfo (project) {
  var dir = Path.join(project, 'Ableton Project Info')
  var from = Path.join(__dirname, 'Project8_1.cfg')
  var to = Path.join(dir, 'Project8_1.cfg')
  fs.mkdirSync(dir)
  fs.writeFileSync(to, fs.readFileSync(from))
}

function tryParse (line) {
  try {
    return JSON.parse(line)
  } catch (ex) {
    return undefined
  }
}
