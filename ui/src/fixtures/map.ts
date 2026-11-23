import type { MapData } from "../kinds/Map";

/** The services fixture of board-plugin-map, with every name replaced, shaded and with a style for each kind of line. */
export const services: MapData = {
  "title": "Basin coral delta",
  "sheet": {
    "width": 720,
    "height": 600
  },
  "regions": [
    {
      "id": "north",
      "title": "Fjord grove harbo",
      "x": 20,
      "y": 20,
      "width": 400,
      "height": 250
    },
    {
      "id": "south",
      "title": "Inlet",
      "x": 436,
      "y": 20,
      "width": 264,
      "height": 250
    },
    {
      "id": "east",
      "title": "Juniper kestrel lagoon",
      "x": 20,
      "y": 286,
      "width": 680,
      "height": 150
    },
    {
      "id": "west",
      "title": "Meadow ne",
      "x": 20,
      "y": 452,
      "width": 680,
      "height": 128
    }
  ],
  "items": [
    {
      "id": "alder",
      "region": "north",
      "label": [
        "Amber",
        "basin coral de"
      ],
      "radius": 8,
      "at": {
        "x": 260,
        "y": 130,
        "set": "pinned"
      },
      "side": "above",
      "bin": 0
    },
    {
      "id": "birch",
      "region": "north",
      "label": [
        "Ember fjo",
        "grove harbor inlet"
      ],
      "radius": 8,
      "at": {
        "x": 100,
        "y": 225,
        "set": "pinned"
      },
      "side": "below",
      "bin": 1
    },
    {
      "id": "cedar",
      "region": "north",
      "label": [
        "Junip",
        "kestrel lago"
      ],
      "radius": 8,
      "at": {
        "x": 80,
        "y": 90,
        "set": "pinned"
      },
      "side": "above",
      "bin": 2
    },
    {
      "id": "dogwood",
      "region": "north",
      "label": [
        "Meadow necta",
        "orchard prairie q"
      ],
      "radius": 8,
      "at": {
        "x": 360,
        "y": 170,
        "set": "pinned"
      },
      "side": "below",
      "bin": 0
    },
    {
      "id": "elm",
      "region": "south",
      "label": [
        "Ridge summ",
        "tundra upland val"
      ],
      "radius": 8,
      "at": {
        "x": 590,
        "y": 195,
        "set": "pinned"
      },
      "side": "above",
      "bin": 1
    },
    {
      "id": "fir",
      "region": "east",
      "label": [
        "Wil",
        "yarrow zep"
      ],
      "radius": 8,
      "at": {
        "x": 600,
        "y": 340,
        "set": "pinned"
      },
      "side": "right",
      "bin": 2
    },
    {
      "id": "ginkgo",
      "region": "east",
      "label": [
        "Amber ba",
        "coral del"
      ],
      "radius": 8,
      "at": {
        "x": 420,
        "y": 380,
        "set": "pinned"
      },
      "side": "left",
      "bin": 0
    },
    {
      "id": "hazel",
      "region": "east",
      "label": [
        "Ember fj",
        "grove harborx"
      ],
      "radius": 8,
      "at": {
        "x": 190,
        "y": 360,
        "set": "pinned"
      },
      "side": "left",
      "bin": 1
    },
    {
      "id": "ivy",
      "region": "west",
      "label": [
        "Juniperx",
        "lagoon meadow n"
      ],
      "radius": 8,
      "at": {
        "x": 360,
        "y": 530,
        "set": "pinned"
      },
      "side": "below",
      "bin": 2
    },
    {
      "id": "jasmine",
      "region": "west",
      "label": [
        "Orchard pra",
        "quarry ridge sum"
      ],
      "radius": 8,
      "at": {
        "x": 200,
        "y": 548,
        "set": "pinned"
      },
      "side": "above",
      "bin": 0
    },
    {
      "id": "kapok",
      "region": "west",
      "label": [
        "Tundrax",
        "valley willow"
      ],
      "radius": 8,
      "at": {
        "x": 560,
        "y": 500,
        "set": "pinned"
      },
      "side": "right",
      "bin": 1
    },
    {
      "id": "larch",
      "region": "west",
      "label": [
        "Yarrow zephyr amb",
        "basin coral deltax"
      ],
      "radius": 8,
      "at": {
        "x": 90,
        "y": 525,
        "set": "pinned"
      },
      "side": "below",
      "bin": 2
    }
  ],
  "lines": [
    {
      "from": "alder",
      "to": "ivy",
      "kind": "lib"
    },
    {
      "from": "ginkgo",
      "to": "ivy",
      "kind": "lib"
    },
    {
      "from": "fir",
      "to": "ivy",
      "kind": "lib"
    },
    {
      "from": "fir",
      "to": "ginkgo",
      "kind": "lib"
    },
    {
      "from": "elm",
      "to": "ivy",
      "kind": "lib"
    },
    {
      "from": "elm",
      "to": "kapok",
      "kind": "lib"
    },
    {
      "from": "hazel",
      "to": "ivy",
      "kind": "lib"
    },
    {
      "from": "hazel",
      "to": "kapok",
      "kind": "lib"
    },
    {
      "from": "jasmine",
      "to": "ivy",
      "kind": "run"
    },
    {
      "from": "cedar",
      "to": "alder",
      "kind": "run"
    },
    {
      "from": "birch",
      "to": "alder",
      "kind": "run"
    },
    {
      "from": "dogwood",
      "to": "alder",
      "kind": "run"
    },
    {
      "from": "dogwood",
      "to": "birch",
      "kind": "run"
    },
    {
      "from": "elm",
      "to": "alder",
      "kind": "run"
    },
    {
      "from": "elm",
      "to": "fir",
      "kind": "run"
    },
    {
      "from": "elm",
      "to": "ginkgo",
      "kind": "run"
    },
    {
      "from": "elm",
      "to": "hazel",
      "kind": "run"
    },
    {
      "from": "hazel",
      "to": "fir",
      "kind": "run"
    },
    {
      "from": "hazel",
      "to": "larch",
      "kind": "plan"
    }
  ],
  "notes": [
    {
      "text": "orcha",
      "anchor": {
        "line": {
          "from": "alder",
          "to": "ivy",
          "along": 0.35,
          "offset": 9
        }
      }
    },
    {
      "text": "prair",
      "anchor": {
        "line": {
          "from": "ginkgo",
          "to": "ivy",
          "along": 0.5,
          "offset": -10
        }
      }
    },
    {
      "text": "quarr",
      "anchor": {
        "line": {
          "from": "fir",
          "to": "ivy",
          "along": 0.35,
          "offset": 9
        }
      }
    },
    {
      "text": "ridge",
      "anchor": {
        "line": {
          "from": "elm",
          "to": "ivy",
          "along": 0.35,
          "offset": 9
        }
      }
    },
    {
      "text": "summi",
      "anchor": {
        "line": {
          "from": "hazel",
          "to": "ivy",
          "along": 0.35,
          "offset": 9
        }
      }
    },
    {
      "text": "tundra upland valley wil",
      "anchor": {
        "item": {
          "id": "cedar",
          "dx": -60,
          "dy": 45
        }
      }
    }
  ],
  "$type": "map/layout/v1",
  "ramp": {
    "label": "Open tasks",
    "bins": [
      "none",
      "1 to 3",
      "4+"
    ]
  },
  "line_kinds": [
    {
      "kind": "lib",
      "label": "library",
      "style": "solid"
    },
    {
      "kind": "run",
      "label": "run-time link",
      "style": "dashed",
      "arrow": true
    },
    {
      "kind": "plan",
      "label": "planned",
      "style": "dotted",
      "arrow": false
    }
  ]
};

/** The work fixture of board-plugin-map, with a ramp and a table. */
export const work: MapData = {
  "title": "Basin co",
  "sheet": {
    "width": 720,
    "height": 700
  },
  "regions": [
    {
      "id": "north",
      "title": "Willo",
      "x": 20,
      "y": 20,
      "width": 420,
      "height": 250
    },
    {
      "id": "south",
      "title": "Yarro",
      "x": 456,
      "y": 20,
      "width": 244,
      "height": 250
    },
    {
      "id": "east",
      "title": "Zephyr amber bas",
      "x": 20,
      "y": 286,
      "width": 680,
      "height": 230
    },
    {
      "id": "west",
      "title": "Coralx",
      "x": 20,
      "y": 532,
      "width": 300,
      "height": 150
    },
    {
      "id": "centre",
      "title": "Ember fjord g",
      "x": 336,
      "y": 532,
      "width": 364,
      "height": 150
    }
  ],
  "items": [
    {
      "id": "alder",
      "region": "north",
      "label": [
        "Amber",
        "basin coral delta embe"
      ],
      "radius": 25.7,
      "at": {
        "x": 230,
        "y": 150,
        "set": "pinned"
      },
      "side": "left",
      "bin": 0
    },
    {
      "id": "birch",
      "region": "north",
      "label": [
        "Fjord grove harbo",
        "inlet juniper kestre"
      ],
      "radius": 9.4,
      "at": {
        "x": 285,
        "y": 220,
        "set": "pinned"
      },
      "side": "right",
      "bin": 1
    },
    {
      "id": "cedar",
      "region": "south",
      "label": [
        "Lagoo",
        "meadow nectar orchar"
      ],
      "radius": 9.5,
      "at": {
        "x": 500,
        "y": 150,
        "set": "pinned"
      },
      "side": "right",
      "bin": 0
    },
    {
      "id": "dogwood",
      "region": "east",
      "label": [
        "Pra",
        "quarry ridge summit t"
      ],
      "radius": 22.3,
      "at": {
        "x": 100,
        "y": 450,
        "set": "pinned"
      },
      "side": "right",
      "bin": 1
    },
    {
      "id": "elm",
      "region": "east",
      "label": [
        "Upland v",
        "willow yarrow zephyr"
      ],
      "radius": 16.3,
      "at": {
        "x": 300,
        "y": 450,
        "set": "pinned"
      },
      "side": "right",
      "bin": 0
    },
    {
      "id": "fir",
      "region": "east",
      "label": [
        "Amber ba",
        "coral delta ember fj"
      ],
      "radius": 12.5,
      "at": {
        "x": 500,
        "y": 450,
        "set": "pinned"
      },
      "side": "right",
      "bin": 1
    },
    {
      "id": "ginkgo",
      "region": "east",
      "label": [
        "Grove harb",
        "inlet juniper kestre"
      ],
      "radius": 11.5,
      "at": {
        "x": 300,
        "y": 360,
        "set": "pinned"
      },
      "side": "right",
      "bin": 0
    },
    {
      "id": "hazel",
      "region": "east",
      "label": [
        "Lagoonx",
        "nectar orchard prair"
      ],
      "radius": 10.1,
      "at": {
        "x": 100,
        "y": 360,
        "set": "pinned"
      },
      "side": "right",
      "bin": 1
    },
    {
      "id": "ivy",
      "region": "east",
      "label": [
        "Quarry r",
        "summit tundra upland"
      ],
      "radius": 8.7,
      "at": {
        "x": 500,
        "y": 360,
        "set": "pinned"
      },
      "side": "right",
      "bin": 0
    },
    {
      "id": "jasmine",
      "region": "west",
      "label": [
        "Valley willow yarr",
        "zephyr amber basinx"
      ],
      "radius": 7.4,
      "at": {
        "x": 100,
        "y": 615,
        "set": "pinned"
      },
      "side": "right",
      "bin": 1
    },
    {
      "id": "kapok",
      "region": "centre",
      "label": [
        "Delta emberx",
        "grove harbor inletx"
      ],
      "radius": 9.4,
      "at": {
        "x": 560,
        "y": 590,
        "set": "pinned"
      },
      "side": "right",
      "bin": 0
    },
    {
      "id": "larch",
      "region": "centre",
      "label": [
        "Kestrel lagoon m",
        "nectar orchard pra"
      ],
      "radius": 7.4,
      "at": {
        "x": 560,
        "y": 650,
        "set": "pinned"
      },
      "side": "right",
      "bin": 1
    },
    {
      "id": "maple",
      "region": "centre",
      "label": [
        "Quarry ridge s",
        "tundra upland valley"
      ],
      "radius": 14.2,
      "at": {
        "x": 380,
        "y": 615,
        "set": "pinned"
      },
      "side": "right",
      "bin": 0
    }
  ],
  "lines": [
    {
      "from": "jasmine",
      "to": "dogwood",
      "kind": "wait"
    },
    {
      "from": "ginkgo",
      "to": "alder",
      "kind": "wait"
    },
    {
      "from": "alder",
      "to": "cedar",
      "kind": "wait"
    },
    {
      "from": "birch",
      "to": "alder",
      "kind": "wait"
    }
  ],
  "notes": [
    {
      "text": "harbor inlet juniper kes",
      "anchor": {
        "item": {
          "id": "cedar",
          "dx": -10,
          "dy": 45
        }
      }
    },
    {
      "text": "lagoon meadow nectar orchard prai",
      "anchor": {
        "item": {
          "id": "alder",
          "dx": -60,
          "dy": -45
        }
      }
    },
    {
      "text": "quarry ridge summit tund",
      "anchor": {
        "item": {
          "id": "dogwood",
          "dx": 20,
          "dy": 40
        }
      }
    },
    {
      "text": "upland valley willowx",
      "anchor": {
        "item": {
          "id": "maple",
          "dx": -20,
          "dy": 38
        }
      }
    }
  ],
  "$type": "map/layout/v1",
  "ramp": {
    "label": "Open changes",
    "bins": [
      "none",
      "some"
    ]
  },
  "tables": [
    {
      "id": "by-area",
      "title": "Every area",
      "caption": "Counts are open tasks.",
      "columns": [
        {
          "label": "Area"
        },
        {
          "label": "Open tasks",
          "numeric": true
        }
      ],
      "rows": [
        {
          "item": "alder",
          "cells": [
            "Amber",
            "3"
          ]
        },
        {
          "item": "birch",
          "cells": [
            "Fjord grove harbo",
            "4"
          ]
        },
        {
          "item": "cedar",
          "cells": [
            "Lagoo",
            "5"
          ]
        },
        {
          "item": "dogwood",
          "cells": [
            "Pra",
            "6"
          ]
        }
      ]
    }
  ]
};

/** The queue fixture of board-plugin-map, with every name replaced: items in five lanes, each with its column, shaded by what it waits on. */
export const queue: MapData = {
  "$type": "map/layout/v1",
  "title": "Yarrow zephyr a",
  "sheet": {
    "width": 720,
    "height": 770
  },
  "regions": [
    {
      "id": "north",
      "title": "Prairie quarry ridg",
      "x": 20,
      "y": 20,
      "width": 680,
      "height": 130
    },
    {
      "id": "south",
      "title": "Summit tundra upl",
      "x": 20,
      "y": 160,
      "width": 680,
      "height": 140
    },
    {
      "id": "east",
      "title": "Valley wil",
      "x": 20,
      "y": 410,
      "width": 680,
      "height": 100
    },
    {
      "id": "west",
      "title": "Yarro",
      "x": 20,
      "y": 520,
      "width": 680,
      "height": 230
    },
    {
      "id": "centre",
      "title": "Zephyr amb",
      "x": 20,
      "y": 310,
      "width": 680,
      "height": 90
    }
  ],
  "items": [
    {
      "id": "alder",
      "region": "north",
      "label": [
        "Amber basinx",
        "delta ember fjor"
      ],
      "radius": 8,
      "at": {
        "x": 190,
        "y": 85,
        "set": "pinned"
      },
      "side": "left",
      "lane": "north",
      "column": 0,
      "bin": 0
    },
    {
      "id": "birch",
      "region": "north",
      "label": [
        "Grove ha",
        "inlet juniper ke"
      ],
      "radius": 8,
      "at": {
        "x": 400,
        "y": 60,
        "set": "pinned"
      },
      "side": "right",
      "lane": "north",
      "column": 1,
      "bin": 1
    },
    {
      "id": "cedar",
      "region": "north",
      "label": [
        "Lagoon mea",
        "nectar orchard p"
      ],
      "radius": 8,
      "at": {
        "x": 400,
        "y": 110,
        "set": "pinned"
      },
      "side": "right",
      "lane": "north",
      "column": 1,
      "bin": 2
    },
    {
      "id": "dogwood",
      "region": "south",
      "label": [
        "Quarry rid",
        "summit tundra upla"
      ],
      "radius": 8,
      "at": {
        "x": 190,
        "y": 200,
        "set": "pinned"
      },
      "side": "left",
      "lane": "south",
      "column": 0,
      "bin": 0
    },
    {
      "id": "elm",
      "region": "south",
      "label": [
        "Valley w",
        "yarrow zephyr ambe"
      ],
      "radius": 8,
      "at": {
        "x": 190,
        "y": 255,
        "set": "pinned"
      },
      "side": "left",
      "lane": "south",
      "column": 0,
      "bin": 1
    },
    {
      "id": "fir",
      "region": "south",
      "label": [
        "Basin coral delta",
        "ember fjord grove harbor i"
      ],
      "radius": 8,
      "at": {
        "x": 400,
        "y": 200,
        "set": "pinned"
      },
      "side": "right",
      "lane": "south",
      "column": 1,
      "bin": 2
    },
    {
      "id": "ginkgo",
      "region": "east",
      "label": [
        "Juniper kestrel lago",
        "meadow nectar orch"
      ],
      "radius": 8,
      "at": {
        "x": 190,
        "y": 450,
        "set": "pinned"
      },
      "side": "left",
      "lane": "east",
      "column": 0,
      "bin": 0
    },
    {
      "id": "hazel",
      "region": "east",
      "label": [
        "Prairie quarry r",
        "summit tundra upla"
      ],
      "radius": 8,
      "at": {
        "x": 190,
        "y": 488,
        "set": "pinned"
      },
      "side": "left",
      "lane": "east",
      "column": 0,
      "bin": 1
    },
    {
      "id": "ivy",
      "region": "west",
      "label": [
        "Valley willo",
        "yarrow zephyr amber bas"
      ],
      "radius": 8,
      "at": {
        "x": 190,
        "y": 640,
        "set": "pinned"
      },
      "side": "left",
      "lane": "west",
      "column": 0,
      "bin": 2
    },
    {
      "id": "jasmine",
      "region": "west",
      "label": [
        "Coral delta embe",
        "fjord grove harbor inlet j",
        "kestrel lagoon meadow"
      ],
      "radius": 8,
      "at": {
        "x": 400,
        "y": 585,
        "set": "pinned"
      },
      "side": "above",
      "lane": "west",
      "column": 1,
      "bin": 0
    },
    {
      "id": "kapok",
      "region": "west",
      "label": [
        "Nectar orchard pr",
        "quarry ridge summit tundr"
      ],
      "radius": 8,
      "at": {
        "x": 400,
        "y": 645,
        "set": "pinned"
      },
      "side": "below",
      "lane": "west",
      "column": 1,
      "bin": 1
    },
    {
      "id": "larch",
      "region": "west",
      "label": [
        "Upland valle",
        "willow yarrow zephyr amber"
      ],
      "radius": 8,
      "at": {
        "x": 400,
        "y": 722,
        "set": "pinned"
      },
      "side": "right",
      "lane": "west",
      "column": 1,
      "bin": 2
    },
    {
      "id": "maple",
      "region": "west",
      "label": [
        "Basin coral delta e",
        "fjord grove harbor inle"
      ],
      "radius": 8,
      "at": {
        "x": 530,
        "y": 610,
        "set": "pinned"
      },
      "side": "right",
      "lane": "west",
      "column": 2,
      "bin": 0
    },
    {
      "id": "nutmeg",
      "region": "centre",
      "label": [
        "Juniper kest",
        "lagoon meadow nectar or"
      ],
      "radius": 8,
      "at": {
        "x": 400,
        "y": 355,
        "set": "pinned"
      },
      "side": "right",
      "lane": "centre",
      "column": 0,
      "bin": 1
    }
  ],
  "lines": [
    {
      "from": "birch",
      "to": "alder",
      "kind": "after"
    },
    {
      "from": "cedar",
      "to": "alder",
      "kind": "after"
    },
    {
      "from": "fir",
      "to": "dogwood",
      "kind": "after"
    },
    {
      "from": "jasmine",
      "to": "ivy",
      "kind": "after"
    },
    {
      "from": "kapok",
      "to": "ivy",
      "kind": "after"
    },
    {
      "from": "larch",
      "to": "ivy",
      "kind": "after"
    },
    {
      "from": "maple",
      "to": "jasmine",
      "kind": "after"
    },
    {
      "from": "maple",
      "to": "kapok",
      "kind": "after"
    },
    {
      "from": "nutmeg",
      "to": "dogwood",
      "kind": "after"
    }
  ],
  "notes": [
    {
      "text": "basin coral delta ember fjord grove harbo",
      "anchor": {
        "item": {
          "id": "ivy",
          "dx": -176,
          "dy": 62
        }
      }
    },
    {
      "text": "inlet juniper kestrel lagoon meadow nectar",
      "anchor": {
        "item": {
          "id": "cedar",
          "dx": 14,
          "dy": 34
        }
      }
    },
    {
      "text": "orchard prairie quarry ridge summit t",
      "anchor": {
        "item": {
          "id": "nutmeg",
          "dx": 14,
          "dy": 36
        }
      }
    }
  ],
  "ramp": {
    "label": "Waits on",
    "bins": [
      "nothing",
      "1",
      "2+"
    ]
  },
  "line_kinds": [
    {
      "kind": "after",
      "label": "builds after",
      "style": "solid",
      "arrow": true
    }
  ]
};
